//! Translation from GLSL ES 1.00 (WebGL 1) to desktop GLSL.
//!
//! Desktop drivers accept ES-style shaders in compatibility profiles once the
//! `precision` statements are gone, but core profiles (which macOS always
//! hands out, even for a 2.1 request) need the modern attribute and varying
//! syntax instead.

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    Vertex,
    Fragment,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Profile {
    Compatibility,
    Core,
}

pub fn translate(source: &str, stage: Stage, profile: Profile) -> Result<String, String> {
    if source.contains("#version 300") || source.contains("#version 310") {
        return Err("GLSL ES 3 shaders are not supported by the native runtime".into());
    }

    let body = strip_es_source(source);
    match profile {
        Profile::Compatibility => Ok(format!("#version 120\n{body}")),
        Profile::Core => {
            let mut text = replace_word(&body, "texture2D", "texture");
            match stage {
                Stage::Vertex => {
                    text = replace_word(&text, "attribute", "in");
                    text = replace_word(&text, "varying", "out");
                    Ok(format!("#version 150 core\n{text}"))
                }
                Stage::Fragment => {
                    text = replace_word(&text, "varying", "in");
                    text = replace_word(&text, "gl_FragColor", "kapFragColor");
                    Ok(format!("#version 150 core\nout vec4 kapFragColor;\n{text}"))
                }
            }
        }
    }
}

fn strip_es_source(source: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut chars = source.char_indices().peekable();

    while let Some((index, character)) = chars.next() {
        if character == '#' {
            let rest = &source[index..];
            if rest.starts_with("#version 100")
                || rest.starts_with("#extension")
                || rest.starts_with("#define GL_ES")
            {
                for (_, next) in chars.by_ref() {
                    if next == '\n' {
                        break;
                    }
                }
                output.push('\n');
                continue;
            }
        }

        if is_word_start(source, index, character, "precision") {
            let mut ended = false;
            for (_, next) in chars.by_ref() {
                if next == ';' {
                    ended = true;
                    break;
                }
            }
            if ended {
                continue;
            }
        }

        output.push(character);
    }

    output
}

fn replace_word(source: &str, word: &str, replacement: &str) -> String {
    let mut output = String::with_capacity(source.len());
    let mut index = 0;
    while index < source.len() {
        if source[index..].starts_with(word)
            && is_word_start(source, index, source[index..].chars().next().unwrap(), word)
        {
            output.push_str(replacement);
            index += word.len();
            continue;
        }
        let character = source[index..].chars().next().unwrap();
        output.push(character);
        index += character.len_utf8();
    }
    output
}

fn is_word_start(source: &str, index: usize, character: char, word: &str) -> bool {
    if !character.is_ascii_alphabetic() {
        return false;
    }
    if !source[index..].starts_with(word) {
        return false;
    }
    let before = source[..index].chars().next_back();
    if before.is_some_and(|value| value.is_ascii_alphanumeric() || value == '_') {
        return false;
    }
    let after = source[index + word.len()..].chars().next();
    !after.is_some_and(|value| value.is_ascii_alphanumeric() || value == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn strips_precision_statements() {
        let translated = translate(
            "precision mediump float;void main(){gl_FragColor=vec4(1.0);}",
            Stage::Fragment,
            Profile::Compatibility,
        )
        .unwrap();
        assert_eq!(
            translated,
            "#version 120\nvoid main(){gl_FragColor=vec4(1.0);}"
        );
    }

    #[test]
    fn keeps_identifiers_containing_precision() {
        let translated = translate(
            "float precisionValue;",
            Stage::Vertex,
            Profile::Compatibility,
        )
        .unwrap();
        assert_eq!(translated, "#version 120\nfloat precisionValue;");
    }

    #[test]
    fn requires_a_desktop_version_directive() {
        let translated = translate(
            "#version 100\nattribute vec2 a;",
            Stage::Vertex,
            Profile::Compatibility,
        )
        .unwrap();
        assert!(translated.starts_with("#version 120\n"));
        assert_eq!(translated.matches("#version").count(), 1);
        assert!(translated.contains("attribute vec2 a"));
    }

    #[test]
    fn drops_extension_directives() {
        let translated = translate(
            "#extension GL_OES_standard_derivatives : enable\nvarying vec2 v;",
            Stage::Fragment,
            Profile::Compatibility,
        )
        .unwrap();
        assert!(!translated.contains("#extension"));
        assert!(translated.contains("varying vec2 v;"));
    }

    #[test]
    fn rewrites_core_vertex_shaders() {
        let translated = translate(
            "#version 100\nprecision highp float;attribute vec2 a;varying vec2 v;void main(){v=a;}",
            Stage::Vertex,
            Profile::Core,
        )
        .unwrap();
        assert!(translated.starts_with("#version 150 core\n"));
        assert!(translated.contains("in vec2 a;"));
        assert!(translated.contains("out vec2 v;"));
        assert!(!translated.contains("precision"));
    }

    #[test]
    fn rewrites_core_fragment_shaders() {
        let translated = translate(
            "precision mediump float;varying vec2 v;uniform sampler2D t;void main(){gl_FragColor=texture2D(t,v);}",
            Stage::Fragment,
            Profile::Core,
        )
        .unwrap();
        assert!(translated.starts_with("#version 150 core\nout vec4 kapFragColor;"));
        assert!(translated.contains("in vec2 v;"));
        assert!(translated.contains("kapFragColor=texture(t,v);"));
    }

    #[test]
    fn keeps_words_that_contain_glsl_keywords() {
        let translated =
            translate("float mytexture2Dvarying;", Stage::Fragment, Profile::Core).unwrap();
        assert!(translated.contains("mytexture2Dvarying"));
    }

    #[test]
    fn rejects_es3() {
        assert!(translate(
            "#version 300 es\nvoid main(){}",
            Stage::Vertex,
            Profile::Core
        )
        .is_err());
    }
}
