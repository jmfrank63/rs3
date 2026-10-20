//! rs3 v0 wire format: the `/insert` request body, its echo and the `/list` output.
//!
//! Shared by the HTTP layer and, from Phase 3, the JS ops, so both speak one codec.
use super::StoreError;

use std::fmt;

use serde::de::{DeserializeSeed, IgnoredAny, MapAccess, Visitor};
use serde_json::error::Category;
use serde_json::{Map, Value};

/// Parse an insert body: a JSON object whose values are all strings.
///
/// Returns the pairs in body order (serde_json `preserve_order` is enabled).
/// Classification (400 vs 422):
/// - `MalformedJson` (400): text that does not parse as JSON strings, numbers,
///   arrays and objects. That covers a syntax error, an empty or truncated body,
///   trailing characters, and a string (key or value) whose `\uXXXX` escapes
///   encode an unpaired UTF-16 surrogate (`"\ud800"`, `"\udc00"`,
///   `"\ud800\u0041"`): such a string has no Unicode text, so it is treated as a
///   string-syntax error, whether it is a key or a value.
/// - `InvalidInput` (422): valid JSON of the wrong shape, naming the offending
///   key. That is a body that is not an object, an object with a non-string
///   value, and valid JSON that serde_json cannot build into a `Value`: a number
///   outside the `f64` range (`{"a": 1e400}`) or nesting past serde_json's
///   recursion limit. Those are reported against their key, or as "not a JSON
///   object" when they are the whole body.
///
/// When a body has several problems, a syntax error is reported first; then the
/// first string that cannot be decoded or value that cannot be represented, in
/// body order; only if there is none is the first non-string value reported.
/// With duplicate keys the last occurrence wins (serde_json map
/// semantics), so only the last value must be a string. An earlier occurrence
/// that cannot be parsed or represented at all (`{"a":1e400,"a":"1"}`) still
/// rejects the whole body.
pub fn parse_insert_body(body: &str) -> Result<Vec<(String, String)>, StoreError> {
    // Pass 1: syntax only. `IgnoredAny` scans numbers without converting them and
    // has no recursion limit, so only real syntax, EOF or I/O errors fail here.
    // It also skips `\uXXXX` escapes without checking surrogate pairing.
    serde_json::from_str::<IgnoredAny>(body).map_err(|e| match e.classify() {
        // Defensive: `IgnoredAny` never reports a data error.
        Category::Data => StoreError::InvalidInput(e.to_string()),
        Category::Syntax | Category::Eof | Category::Io => StoreError::MalformedJson(e.to_string()),
    })?;
    // Pass 2: the text is valid JSON grammar, so a failure here is either an
    // unpaired surrogate escape (400) or a value serde_json cannot represent
    // (422, reported against its key).
    let value: Value = serde_json::from_str(body).map_err(|e| {
        if is_surrogate_escape_error(&e) {
            StoreError::MalformedJson(format!(
                "string contains an unpaired UTF-16 surrogate \\u escape: {e}"
            ))
        } else {
            unrepresentable(body, &e)
        }
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

/// Whether a pass-2 failure is a `\uXXXX` escape with an unpaired UTF-16 surrogate.
///
/// After pass 1 accepted the text, serde_json 1.0 fails to build a `Value` for
/// only three reasons: `number out of range`, `recursion limit exceeded`, or a
/// surrogate escape it cannot decode into a `String` (`lone leading surrogate in
/// hex escape` or `unexpected end of hex escape`). serde_json exposes no error
/// code, so the message text tells them apart: only the surrogate errors
/// mention an escape. The `parse_unpaired_surrogate_*` tests pin this.
fn is_surrogate_escape_error(error: &serde_json::Error) -> bool {
    error.to_string().contains("escape")
}

/// The `InvalidInput` error for valid JSON that serde_json could not build into a `Value`.
///
/// Only a number out of range or nesting past the recursion limit reaches here.
/// Both fail inside a value (object keys are strings, and string failures are
/// handled before this is called), so for an object the key is always found.
/// When no key is found, the body is not an object.
fn unrepresentable(body: &str, error: &serde_json::Error) -> StoreError {
    let mut bad_key = None;
    let mut de = serde_json::Deserializer::from_str(body);
    // The result is irrelevant: only the key recorded before the failure matters.
    let _ = FindBadKey(&mut bad_key).deserialize(&mut de);
    match bad_key {
        Some(key) => {
            StoreError::InvalidInput(format!("value for key {key} must be a string: {error}"))
        }
        None => StoreError::InvalidInput(format!("insert body must be a JSON object: {error}")),
    }
}

/// Walks a top-level JSON object, recording each key before building its value,
/// so after a failure the slot holds the key whose value could not be built.
struct FindBadKey<'a>(&'a mut Option<String>);

impl<'de> DeserializeSeed<'de> for FindBadKey<'_> {
    type Value = ();

    fn deserialize<D: serde::Deserializer<'de>>(self, deserializer: D) -> Result<(), D::Error> {
        deserializer.deserialize_map(self)
    }
}

impl<'de> Visitor<'de> for FindBadKey<'_> {
    type Value = ();

    fn expecting(&self, f: &mut fmt::Formatter) -> fmt::Result {
        f.write_str("a JSON object")
    }

    fn visit_map<A: MapAccess<'de>>(self, mut map: A) -> Result<(), A::Error> {
        while let Some(key) = map.next_key::<String>()? {
            *self.0 = Some(key);
            map.next_value::<Value>()?;
            *self.0 = None;
        }
        Ok(())
    }
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

/// Format entries as the rs3 v0 `/list` text: `[{"k" : "v"},{"k2" : "v2"}]`.
///
/// Keeps the legacy `" : "` spacing and `,` separators, but every key and value
/// is JSON-escaped by serde_json, so the text is always valid JSON that parses
/// back to exactly the given pairs, and a value can never forge an extra entry.
/// For data that needs no escaping the output is byte-identical to legacy. An
/// empty slice gives `[]`. Entries arrive sorted from `Store::list`. This one
/// codec is shared by `GET /list` and, from Phase 3, the `op_rs3_list` JS op.
pub fn format_list(entries: &[(String, String)]) -> String {
    let items: Vec<String> = entries
        .iter()
        .map(|(k, v)| {
            format!(
                "{{{} : {}}}",
                Value::from(k.as_str()),
                Value::from(v.as_str())
            )
        })
        .collect();
    format!("[{}]", items.join(","))
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
    fn parse_rejects_truncated_deep_nesting() {
        assert!(is_malformed(parse_insert_body(&"[".repeat(200))));
    }

    #[test]
    fn parse_unrepresentable_value_beats_earlier_non_string() {
        match parse_insert_body(r#"{"a":1,"b":1e400}"#) {
            Err(StoreError::InvalidInput(m)) => assert!(m.contains("key b"), "message: {m}"),
            other => panic!("expected InvalidInput naming key b, got {other:?}"),
        }
    }

    #[test]
    fn parse_out_of_range_number_is_invalid_input_naming_key() {
        for body in [r#"{"a": 1e400}"#, r#"{"ok":"1","a":-1e400}"#] {
            match parse_insert_body(body) {
                Err(StoreError::InvalidInput(m)) => assert!(m.contains("key a"), "message: {m}"),
                other => panic!("{body}: expected InvalidInput, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_out_of_range_number_body_is_invalid_input() {
        assert!(is_invalid_input(parse_insert_body("1e400")));
        assert!(is_invalid_input(parse_insert_body("[1e400]")));
    }

    #[test]
    fn parse_valid_deep_nesting_is_invalid_input() {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        assert!(is_invalid_input(parse_insert_body(&deep)));
        match parse_insert_body(&format!(r#"{{"ok":"1","a":{deep}}}"#)) {
            Err(StoreError::InvalidInput(m)) => assert!(m.contains("key a"), "message: {m}"),
            other => panic!("expected InvalidInput, got {other:?}"),
        }
    }

    #[test]
    fn parse_syntax_error_next_to_big_number_is_malformed() {
        assert!(is_malformed(parse_insert_body(r#"{"a": 1e400"#)));
        assert!(is_malformed(parse_insert_body(r#"{"a": 1e}"#)));
    }

    /// Assert `body` is `MalformedJson` with a message about a surrogate escape.
    fn assert_surrogate_malformed(body: &str) {
        match parse_insert_body(body) {
            Err(StoreError::MalformedJson(m)) => {
                assert!(
                    m.contains("unpaired UTF-16 surrogate"),
                    "{body}: message: {m}"
                );
                assert!(!m.contains("must be a string"), "{body}: message: {m}");
                assert!(!m.contains("JSON object"), "{body}: message: {m}");
            }
            other => panic!("{body}: expected MalformedJson, got {other:?}"),
        }
    }

    #[test]
    fn parse_unpaired_surrogate_in_value_is_malformed() {
        for body in [
            r#"{"a":"\ud800"}"#,
            r#"{"a":"\udc00"}"#,
            r#"{"ok":"1","a":"x\udc00y"}"#,
            r#"{"a":"\ud800x"}"#,
            r#"{"a":"\ud800\u0041"}"#,
            r#"{"a":"\ud800\ud800"}"#,
            r#"{"a":"\ud800\n"}"#,
            r#"{"a":"\ud800\""}"#,
        ] {
            assert_surrogate_malformed(body);
        }
    }

    #[test]
    fn parse_unpaired_surrogate_in_key_is_malformed() {
        for body in [
            r#"{"\ud800":"v"}"#,
            r#"{"\udc00":"v"}"#,
            r#"{"ok":"1","\ud800":"v"}"#,
        ] {
            assert_surrogate_malformed(body);
        }
    }

    #[test]
    fn parse_unpaired_surrogate_nested_or_whole_body_is_malformed() {
        assert_surrogate_malformed(r#"{"a":["\ud800"]}"#);
        assert_surrogate_malformed(r#""\ud800""#);
    }

    #[test]
    fn parse_paired_surrogate_escape_is_accepted() {
        assert_eq!(
            parse_insert_body(r#"{"\ud83d\ude00":"\ud83d\ude00"}"#),
            Ok(vec![("\u{1F600}".to_owned(), "\u{1F600}".to_owned())])
        );
    }

    #[test]
    fn parse_unrepresentable_messages_never_mention_an_escape() {
        let deep = format!("{}{}", "[".repeat(200), "]".repeat(200));
        for body in [
            r#"{"a": 1e400}"#.to_owned(),
            "1e400".to_owned(),
            deep.clone(),
            format!(r#"{{"a":{deep}}}"#),
        ] {
            match parse_insert_body(&body) {
                Err(StoreError::InvalidInput(m)) => {
                    assert!(!m.contains("escape"), "message: {m}");
                }
                other => panic!("expected InvalidInput, got {other:?}"),
            }
        }
    }

    #[test]
    fn parse_duplicate_key_checks_only_last_value_type() {
        assert_eq!(
            parse_insert_body(r#"{"a":1,"a":"2"}"#),
            Ok(vec![("a".to_owned(), "2".to_owned())])
        );
        assert!(is_invalid_input(parse_insert_body(r#"{"a":"1","a":2}"#)));
    }

    #[test]
    fn parse_duplicate_key_unrepresentable_earlier_value_is_rejected() {
        match parse_insert_body(r#"{"a":1e400,"a":"1"}"#) {
            Err(StoreError::InvalidInput(m)) => assert!(m.contains("key a"), "message: {m}"),
            other => panic!("expected InvalidInput, got {other:?}"),
        }
        assert!(is_malformed(parse_insert_body(r#"{"a":"\ud800","a":"1"}"#)));
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
