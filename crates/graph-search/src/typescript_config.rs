//! Native bounded JSONC projection. No compiler or runtime configuration is executed.
use graph_search_core::ports::SourceFile;
use graph_search_types::typescript::TypeScriptConfig;
use serde_json::Value;

/// Blank comments/BOM and trailing commas while leaving quoted bytes untouched.
/// The existing JSON decoder remains authoritative for JSON grammar and escapes.
fn jsonc(text: &str) -> Result<(Value, Vec<String>), &'static str> {
    let mut bytes = text.as_bytes().to_vec();
    if bytes.starts_with(&[0xef, 0xbb, 0xbf]) {
        bytes[..3].fill(b' ');
    }
    let mut cursor = 0;
    let mut string = false;
    while cursor < bytes.len() {
        match bytes[cursor] {
            b'\\' if string => cursor = cursor.saturating_add(2),
            b'"' => {
                string = !string;
                cursor = cursor.saturating_add(1);
            }
            b'/' if !string && bytes.get(cursor.saturating_add(1)) == Some(&b'/') => {
                while cursor < bytes.len() && !matches!(bytes[cursor], b'\n' | b'\r') {
                    bytes[cursor] = b' ';
                    cursor = cursor.saturating_add(1);
                }
            }
            b'/' if !string && bytes.get(cursor.saturating_add(1)) == Some(&b'*') => {
                let start = cursor;
                cursor = cursor.saturating_add(2);
                while cursor.saturating_add(1) < bytes.len()
                    && &bytes[cursor..cursor.saturating_add(2)] != b"*/"
                {
                    cursor = cursor.saturating_add(1);
                }
                if cursor.saturating_add(1) >= bytes.len() {
                    return Err("unterminated_config_comment");
                }
                cursor = cursor.saturating_add(2);
                for byte in &mut bytes[start..cursor] {
                    if !matches!(*byte, b'\n' | b'\r') {
                        *byte = b' ';
                    }
                }
            }
            _ => cursor = cursor.saturating_add(1),
        }
    }
    cursor = 0;
    string = false;
    let mut previous = None;
    while cursor < bytes.len() {
        let byte = bytes[cursor];
        if string && byte == b'\\' {
            cursor = cursor.saturating_add(2);
            continue;
        }
        if byte == b'"' {
            string = !string;
        }
        if !string && byte == b',' {
            let mut next = cursor.saturating_add(1);
            while bytes.get(next).is_some_and(u8::is_ascii_whitespace) {
                next = next.saturating_add(1);
            }
            // Do not turn array holes or missing object values into valid JSON.
            if matches!(bytes.get(next), Some(b']' | b'}'))
                && previous.is_some_and(|byte| !matches!(byte, b'[' | b'{' | b',' | b':'))
            {
                bytes[cursor] = b' ';
            }
        }
        if !byte.is_ascii_whitespace() {
            previous = Some(byte);
        }
        cursor = cursor.saturating_add(1);
    }
    let value: Value = serde_json::from_slice(&bytes).map_err(|_| "invalid_config_syntax")?;
    let patterns = if value
        .get("compilerOptions")
        .and_then(|options| options.get("paths"))
        .and_then(Value::as_object)
        .is_some_and(|paths| paths.keys().any(|key| key.contains('*')))
    {
        crate::typescript_order::patterns(&bytes).map_err(|_| "invalid_config_order")?
    } else {
        Vec::new()
    };
    Ok((value, patterns))
}

pub(crate) fn extract(file: &SourceFile<'_>) -> Option<TypeScriptConfig> {
    if !file
        .path
        .extension()
        .is_some_and(|ext| ext == "json" || ext == "jsonc")
    {
        return None;
    }
    let parsed = (|| {
        if file.text.len() > graph_search_types::limits::MAX_TYPESCRIPT_CONFIG_BYTES {
            return Err("config_byte_limit");
        }
        let (value, path_patterns) = jsonc(file.text)?;
        let Value::Object(mut object) = value else {
            return Err("config_root_not_object");
        };
        let config = TypeScriptConfig {
            fields: TypeScriptConfig::FIELDS
                .into_iter()
                .filter_map(|key| object.remove(key).map(|value| (key.into(), value)))
                .collect(),
            unavailable_reason: None,
            path_patterns,
        };
        if !config.valid() {
            return Err("config_fact_limit");
        }
        Ok(config)
    })();
    Some(parsed.unwrap_or_else(|reason: &str| TypeScriptConfig {
        unavailable_reason: Some(reason.into()),
        ..TypeScriptConfig::default()
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn comments_bom_trailing_commas_and_quoted_delimiters_preserve_authored_values() {
        let text = "\u{feff}{/* café */\"extends\":[\"../one.json\",\"../two.json\",],// note\r\n\"compilerOptions\":{\"paths\":{\"$lib/*\":[\"../src/*\",],},\"baseUrl\":\"https://x/*y*/\",},\"files\":[],}";
        let (value, patterns) = jsonc(text).unwrap();
        assert_eq!(patterns, ["$lib/*"]);
        assert_eq!(
            value,
            serde_json::json!({"extends":["../one.json","../two.json"],"compilerOptions":{"paths":{"$lib/*":["../src/*"]},"baseUrl":"https://x/*y*/"},"files":[]})
        );
        assert_eq!(
            jsonc(r#"{"x":"escaped\"//literal", "y":"\\",}"#).unwrap().0,
            serde_json::json!({"x":"escaped\"//literal","y":"\\"})
        );
        for text in [
            "[,]",
            "[1,,]",
            "{,}",
            r#"{"x":,}"#,
            "/*",
            "{} /* unfinished",
            "{x:1}",
            "{'x':1}",
            "{\"x\":undefined}",
            "[1 2]",
            "{} false",
        ] {
            assert!(jsonc(text).is_err(), "{text}");
        }
    }
    #[test]
    fn projection_keeps_empty_and_invalid_option_values_without_claiming_semantics() {
        let extract = |text| {
            super::extract(&SourceFile {
                path: std::path::Path::new("base.json"),
                text,
            })
            .unwrap()
        };
        let fact = extract(
            r#"{"compilerOptions":{"paths":null,"moduleResolution":7},"files":[],"references":[{"path":"../other","prepend":true}],"unrelated":"discard"}"#,
        );
        assert!(fact.valid());
        assert_eq!(fact.fields.len(), 3);
        assert!(fact.fields["compilerOptions"]["paths"].is_null());
        assert!(fact.fields["references"][0]["prepend"].as_bool().unwrap());
        assert!(extract("{}").fields.is_empty());
        for text in [
            "[]",
            "/*bad",
            &" ".repeat(graph_search_types::limits::MAX_TYPESCRIPT_CONFIG_BYTES + 1),
            &format!(r#"{{"files":["{}"]}}"#, "x".repeat(4097)),
        ] {
            let fact = extract(text);
            assert!(fact.valid() && fact.unavailable_reason.is_some() && fact.fields.is_empty());
        }
    }
}
