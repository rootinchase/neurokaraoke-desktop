#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn test_maybe_artists_deserialization() {
        let j = json!([{"name": "Artist 1"}]);
        let result: Result<MaybeArtists, _> = serde_json::from_value(j);
        assert!(result.is_ok(), "Failed to deserialize: {:?}", result.err());
    }
}
