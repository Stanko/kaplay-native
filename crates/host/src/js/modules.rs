use kaplay_native_pak::normalize;

pub const MODULE_ORIGIN: &str = "kaplay-native://game/";

#[cfg(test)]
pub fn test_assets(files: &[(&str, &str)]) -> std::rc::Rc<kaplay_native_pak::AssetSource> {
    use std::io::Write;
    let mut writer = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
    for (name, source) in files {
        writer
            .start_file(*name, zip::write::SimpleFileOptions::default())
            .unwrap();
        writer.write_all(source.as_bytes()).unwrap();
    }
    std::rc::Rc::new(
        kaplay_native_pak::AssetSource::from_pak_bytes(writer.finish().unwrap().into_inner())
            .unwrap(),
    )
}

pub fn resolve(base: &str, specifier: &str) -> Result<String, String> {
    let base = base.strip_prefix(MODULE_ORIGIN).unwrap_or(base);
    let absolute = specifier.starts_with(MODULE_ORIGIN);
    let specifier = specifier.strip_prefix(MODULE_ORIGIN).unwrap_or(specifier);
    if specifier.contains(':') || specifier.starts_with("//") {
        return Err(format!("unsupported module URL: {specifier}"));
    }
    let path = if specifier.starts_with('.') {
        let parent = base.rsplit_once('/').map_or("", |(parent, _)| parent);
        format!("{parent}/{specifier}")
    } else if absolute || specifier.starts_with('/') {
        specifier.to_string()
    } else {
        return Err(format!(
            "unsupported bare module import '{specifier}' from '{base}'"
        ));
    };
    normalize(&path)
        .filter(|path| !path.is_empty())
        .ok_or_else(|| format!("module path escapes the game directory: {path}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_chunks_relative_to_the_importer() {
        assert_eq!(
            resolve("assets/main.js", "./chunk.js").unwrap(),
            "assets/chunk.js"
        );
        assert_eq!(
            resolve("assets/sub/main.js", "../chunk.js?v=1").unwrap(),
            "assets/chunk.js"
        );
        assert_eq!(
            resolve("assets/main.js", "/shared.js").unwrap(),
            "shared.js"
        );
        assert_eq!(
            resolve("assets/main.js", "kaplay-native://game/assets/chunk.js").unwrap(),
            "assets/chunk.js"
        );
    }

    #[test]
    fn rejects_imports_outside_the_game() {
        for specifier in [
            "../../secret.js",
            "https://example.com/a.js",
            "//example.com/a.js",
            "kaplay",
        ] {
            assert!(resolve("assets/main.js", specifier).is_err(), "{specifier}");
        }
    }
}
