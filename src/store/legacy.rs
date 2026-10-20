//! rs3 v0 wire format: the `/insert` request body and its echo.
//!
//! Shared by the HTTP layer and, from Phase 3, the JS ops, so both speak one codec.
use super::StoreError;

// RED stubs: signatures only, replaced by the GREEN implementation.

/// Parse an insert body: a JSON object whose values are all strings.
pub fn parse_insert_body(_body: &str) -> Result<Vec<(String, String)>, StoreError> {
    Ok(Vec::new())
}

/// Format inserted entries as the compact JSON object text the legacy route echoed.
pub fn format_insert_echo(_entries: &[(String, String)]) -> String {
    String::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn is_invalid_input(r: Result<Vec<(String, String)>, StoreError>) -> bool {
        matches!(r, Err(StoreError::InvalidInput(_)))
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
        assert!(is_invalid_input(parse_insert_body(r#"{"a":"#)));
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
