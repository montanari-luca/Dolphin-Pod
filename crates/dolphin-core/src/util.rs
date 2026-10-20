pub fn new_id() -> String {
    uuid::Uuid::now_v7().to_string()
}

/// A stable id derived from a name, so every device that seeds the same default gets the same id and
/// synced copies of it line up instead of duplicating.
pub fn stable_id(name: &str) -> String {
    const NAMESPACE: uuid::Uuid = uuid::Uuid::from_bytes(*b"dolphinpod.seeds");
    uuid::Uuid::new_v5(&NAMESPACE, name.as_bytes()).to_string()
}

pub fn now_ms() -> i64 {
    chrono::Utc::now().timestamp_millis()
}

/// Truncate to at most `max` chars on a char boundary, adding an ellipsis.
pub fn truncate(s: &str, max: usize) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    let mut out: String = s.chars().take(max).collect();
    out.push('…');
    out
}

/// Pull the first balanced JSON object out of a model reply (handles ```json fences and chatter).
pub fn extract_json_object(text: &str) -> Option<&str> {
    let start = text.find('{')?;
    let bytes = text.as_bytes();
    let (mut depth, mut in_str, mut esc) = (0i32, false, false);
    for (i, &b) in bytes.iter().enumerate().skip(start) {
        if in_str {
            match b {
                _ if esc => esc = false,
                b'\\' => esc = true,
                b'"' => in_str = false,
                _ => {}
            }
            continue;
        }
        match b {
            b'"' => in_str = true,
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(&text[start..=i]);
                }
            }
            _ => {}
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extracts_json_from_fenced_reply() {
        let t = "Sure!\n```json\n{\"a\": {\"b\": \"}\"}, \"c\": 1}\n```";
        assert_eq!(extract_json_object(t), Some("{\"a\": {\"b\": \"}\"}, \"c\": 1}"));
        assert_eq!(extract_json_object("no json"), None);
    }

    #[test]
    fn truncates_on_char_boundary() {
        assert_eq!(truncate("héllo", 2), "hé…");
        assert_eq!(truncate("hi", 5), "hi");
    }
}
