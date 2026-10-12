use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum PakError {
    #[error("failed to read {path}: {source}")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("invalid pak archive: {0}")]
    Zip(#[from] zip::result::ZipError),
    #[error("entry name escapes the archive root: {0}")]
    UnsafeEntry(String),
}

#[derive(Debug, Clone)]
pub struct AssetSource {
    root: PathBuf,
    files: HashMap<String, Vec<u8>>,
    directory: bool,
}

impl AssetSource {
    pub fn from_pak(path: &Path) -> Result<Self, PakError> {
        let bytes = std::fs::read(path).map_err(|source| PakError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(bytes))?;
        let mut files = HashMap::new();

        for index in 0..archive.len() {
            let mut entry = archive.by_index(index)?;
            if entry.is_dir() {
                continue;
            }
            let raw_name = entry.name()?.into_owned();
            let Some(name) = normalize(&raw_name) else {
                return Err(PakError::UnsafeEntry(raw_name));
            };
            if name.is_empty() {
                continue;
            }
            let mut data = Vec::with_capacity(entry.size() as usize);
            entry
                .read_to_end(&mut data)
                .map_err(|source| PakError::Io {
                    path: path.to_path_buf(),
                    source,
                })?;
            files.insert(name, data);
        }

        Ok(Self {
            root: PathBuf::new(),
            files,
            directory: false,
        })
    }

    pub fn from_dir(root: &Path) -> Self {
        Self {
            root: root.to_path_buf(),
            files: HashMap::new(),
            directory: true,
        }
    }

    pub fn is_directory(&self) -> bool {
        self.directory
    }

    pub fn exists(&self, path: &str) -> bool {
        let Some(key) = normalize(path) else {
            return false;
        };
        if self.directory {
            resolve_within(&self.root, &key).is_some_and(|path| path.is_file())
        } else {
            self.files.contains_key(&key)
        }
    }

    pub fn read(&self, path: &str) -> Option<Vec<u8>> {
        let key = normalize(path)?;
        if self.directory {
            let full = resolve_within(&self.root, &key)?;
            std::fs::read(full).ok()
        } else {
            self.files.get(&key).cloned()
        }
    }

    pub fn read_text(&self, path: &str) -> Option<String> {
        self.read(path)
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    pub fn file_count(&self) -> usize {
        self.files.len()
    }
}

pub fn normalize(path: &str) -> Option<String> {
    let mut clean = path.replace('\\', "/");
    if let Some(query) = clean.find(['?', '#']) {
        clean.truncate(query);
    }
    let clean = clean.trim_start_matches('/');
    let mut parts: Vec<&str> = Vec::new();

    for part in clean.split('/') {
        match part {
            "" | "." => continue,
            ".." => {
                parts.pop()?;
            }
            part => parts.push(part),
        }
    }

    Some(parts.join("/"))
}

fn resolve_within(root: &Path, relative: &str) -> Option<PathBuf> {
    let candidate = root.join(relative);
    let root = root.canonicalize().ok()?;
    let canonical = candidate.canonicalize().ok()?;
    canonical.starts_with(&root).then_some(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    #[test]
    fn normalizes_paths() {
        assert_eq!(normalize("/sprites/bean.png").unwrap(), "sprites/bean.png");
        assert_eq!(normalize("./a/./b/../c.png").unwrap(), "a/c.png");
        assert_eq!(normalize("a//b\\c.png").unwrap(), "a/b/c.png");
        assert_eq!(
            normalize("sprites/bean.png?v=2").unwrap(),
            "sprites/bean.png"
        );
        assert_eq!(normalize("a/b.png#frag").unwrap(), "a/b.png");
        assert_eq!(normalize("../escape.png"), None);
        assert_eq!(normalize("a/../../escape.png"), None);
    }

    #[test]
    fn reads_from_directory() {
        let dir = std::env::temp_dir().join(format!("kap-pak-{}", std::process::id()));
        std::fs::create_dir_all(dir.join("sprites")).unwrap();
        std::fs::write(dir.join("sprites/bean.png"), b"png").unwrap();

        let assets = AssetSource::from_dir(&dir);
        assert!(assets.is_directory());
        assert_eq!(assets.read("sprites/bean.png").unwrap(), b"png");
        assert_eq!(assets.read_text("sprites/bean.png").unwrap(), "png");
        assert!(!assets.exists("nope.png"));
        assert!(assets.read("../escape.png").is_none());

        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn reads_from_pak() {
        let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        writer.start_file("game/main.js", options).unwrap();
        writer.write_all(b"console.log(1)").unwrap();
        writer.start_file("assets/bean.png", options).unwrap();
        writer.write_all(b"png").unwrap();
        let bytes = writer.finish().unwrap().into_inner();

        let dir = std::env::temp_dir().join(format!("kap-pak-zip-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let pak_path = dir.join("game.pak");
        std::fs::write(&pak_path, bytes).unwrap();

        let assets = AssetSource::from_pak(&pak_path).unwrap();
        assert!(!assets.is_directory());
        assert_eq!(assets.file_count(), 2);
        assert_eq!(assets.read("assets/bean.png").unwrap(), b"png");
        assert!(assets.exists("game/main.js"));
        assert!(assets.read("missing.js").is_none());

        std::fs::remove_dir_all(&dir).ok();
    }
}
