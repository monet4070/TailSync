pub(super) fn is_structured_json(value: &str) -> bool {
    if !matches!(value.as_bytes().first(), Some(b'{') | Some(b'[')) {
        return false;
    }
    matches!(
        serde_json::from_str::<serde_json::Value>(value),
        Ok(serde_json::Value::Object(_) | serde_json::Value::Array(_))
    )
}
