pub fn translate(source: &str) -> Result<String, String> {
    if source.contains("#version 300") || source.contains("#version 310") {
        return Err("GLSL ES 3 shaders are not supported by the native runtime".into());
    }

    let mut output = String::with_capacity(source.len());
    let mut chars = source.char_indices().peekable();

    while let Some((index, character)) = chars.next() {
        if character == '#' {
            let rest = &source[index..];
            if rest.starts_with("#version 100") {
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

    Ok(output)
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
        let translated =
            translate("precision mediump float;void main(){gl_FragColor=vec4(1.0);}").unwrap();
        assert_eq!(translated, "void main(){gl_FragColor=vec4(1.0);}");
    }

    #[test]
    fn keeps_identifiers_containing_precision() {
        let translated = translate("float precisionValue;").unwrap();
        assert_eq!(translated, "float precisionValue;");
    }

    #[test]
    fn strips_version_100_lines() {
        let translated = translate("#version 100\nattribute vec2 a;").unwrap();
        assert!(!translated.contains("#version"));
        assert!(translated.contains("attribute vec2 a"));
    }

    #[test]
    fn rejects_es3() {
        assert!(translate("#version 300 es\nvoid main(){}").is_err());
    }
}
