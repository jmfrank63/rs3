//! rs3 v0 wire format: the `/insert` request body, its echo and the `/list` output.
//!
//! Shared by the HTTP layer and, from Phase 3, the JS ops, so both speak one codec.
use super::StoreError;

use serde_json::error::Category;
use serde_json::{Map, Value};

/// Parse an insert body: a JSON object whose values are all strings.
///
/// Returns the pairs in body order (serde_json `preserve_order` is enabled).
/// Text that is not JSON at all (a syntax error, an empty or truncated body,
/// trailing characters, nesting past serde_json's recursion limit) gives
/// `MalformedJson`. Valid JSON of the wrong shape (not an object, or an object
/// with a non-string value) gives `InvalidInput`, naming the offending key.
pub fn parse_insert_body(body: &str) -> Result<Vec<(String, String)>, StoreError> {
    let value: Value = serde_json::from_str(body).map_err(|e| match e.classify() {
        // Defensive: deserializing into `Value` never reports a data error.
        Category::Data => StoreError::InvalidInput(e.to_string()),
        Category::Syntax | Category::Eof | Category::Io => StoreError::MalformedJson(e.to_string()),
    })?;
    let object = value
        .as_object()
        .ok_or_else(|| StoreError::InvalidInput("insert body must be a JSON object".into()))?;
    object
        .iter()
        .map(|(k, v)| match v.as_str() {
            Some(s) => Ok((k.clone(), s.to_owned())),
            None => Err(StoreError::InvalidInput(format!(
                "value for key {k} must be a string"
            ))),
        })
        .collect()
}

/// Format inserted entries as the compact JSON object text the legacy route echoed.
///
/// Byte-identical to the legacy `entry.to_string()` for every all-string object.
pub fn format_insert_echo(entries: &[(String, String)]) -> String {
    let map: Map<String, Value> = entries
        .iter()
        .map(|(k, v)| (k.clone(), Value::String(v.clone())))
        .collect();
    Value::Object(map).to_string()
}

/// Format entries as the rs3 v0 `/list` text `[{"k" : "v"},...]`; Phase 2 replaces this shape.
pub fn format_list(entries: &[(String, String)]) -> String {
    let items: Vec<String> = entries
        .iter()
        .map(|(k, v)| format!("{{\"{k}\" : \"{v}\"}}"))
        .collect();
    // The legacy handler applied this replacement last; keep it for byte parity.
    format!("[{}]", items.join(",")).replace(",]", "]")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pairs(items: &[(&str, &str)]) -> Vec<(String, String)> {
        items
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn format_list_empty_is_brackets() {
        assert_eq!(format_list(&[]), "[]");
    }

    #[test]
    fn format_list_shape() {
        assert_eq!(
            format_list(&pairs(&[("a", "1"), ("b", "2")])),
            r#"[{"a" : "1"},{"b" : "2"}]"#
        );
    }

    #[test]
    fn format_list_single_entry() {
        assert_eq!(format_list(&pairs(&[("k", "v")])), r#"[{"k" : "v"}]"#);
    }

    #[test]
    fn format_list_escapes_quotes_and_backslashes() {
        assert_eq!(
            format_list(&pairs(&[("q", r#"a"b\c"#)])),
            r#"[{"q" : "a\"b\\c"}]"#
        );
    }

    /// Parse the inner `/list` text back into its pairs, in order.
    fn parse_list(text: &str) -> Vec<(String, String)> {
        let parsed: Vec<Map<String, Value>> = serde_json::from_str(text).expect("valid JSON");
        parsed
            .into_iter()
            .flat_map(|m| {
                m.into_iter()
                    .map(|(k, v)| (k, v.as_str().expect("string value").to_owned()))
            })
            .collect()
    }

    #[test]
    fn format_list_round_trips() {
        let input = pairs(&[
            ("a", "1"),
            ("b", r#"he said "hi""#),
            ("c", r"back\slash"),
            ("d", "x,]"),
            ("e", "},{"),
            ("f", "line\nbreak"),
            ("g", "tab\there"),
            ("h", "ü /"),
        ]);
        assert_eq!(parse_list(&format_list(&input)), input);
    }

    #[test]
    fn format_list_cannot_forge_entries() {
        let input = pairs(&[("f", r#"x"},{"admin" : "true"#)]);
        let back = parse_list(&format_list(&input));
        assert_eq!(back.len(), 1);
        assert_eq!(back, input);
    }

    fn is_invalid_input(r: Result<Vec<(String, String)>, StoreError>) -> bool {
        matches!(r, Err(StoreError::InvalidInput(_)))
    }

    fn is_malformed(r: Result<Vec<(String, String)>, StoreError>) -> bool {
        matches!(r, Err(StoreError::MalformedJson(_)))
    }

    #[test]
    fn parse_object_of_strings() {
        let parsed = parse_insert_body(r#"{"a":"1","b":"2"}"#);
        assert_eq!(
            parsed,
            Ok(vec![
                ("a".to_owned(), "1".to_owned()),
                ("b".to_owned(), "2".to_owned())
            ])
        );
    }

    #[test]
    fn parse_rejects_malformed_json() {
        assert!(is_malformed(parse_insert_body(r#"{"a":"#)));
        assert!(is_malformed(parse_insert_body("{bad")));
    }

    #[test]
    fn parse_rejects_empty_and_whitespace() {
        assert!(is_malformed(parse_insert_body("")));
        assert!(is_malformed(parse_insert_body("   ")));
    }

    #[test]
    fn parse_rejects_trailing_characters() {
        assert!(is_malformed(parse_insert_body(r#"{"a":"1"} trailing"#)));
        assert!(is_malformed(parse_insert_body(r#"{"a":"1",}"#)));
    }

    #[test]
    fn parse_rejects_deep_nesting() {
        assert!(is_malformed(parse_insert_body(&"[".repeat(200))));
    }

    #[test]
    fn parse_null_is_invalid_input() {
        assert!(is_invalid_input(parse_insert_body("null")));
    }

    #[test]
    fn parse_rejects_non_object() {
        assert!(is_invalid_input(parse_insert_body("[1,2]")));
        assert!(is_invalid_input(parse_insert_body(r#""x""#)));
        assert!(is_invalid_input(parse_insert_body("3")));
    }

    #[test]
    fn parse_rejects_non_string_value() {
        assert!(is_invalid_input(parse_insert_body(r#"{"a":1}"#)));
    }

    #[test]
    fn echo_matches_legacy_bytes() {
        let entries = vec![("insert_test".to_owned(), "John Doe".to_owned())];
        assert_eq!(
            format_insert_echo(&entries),
            r#"{"insert_test":"John Doe"}"#
        );
    }
}
