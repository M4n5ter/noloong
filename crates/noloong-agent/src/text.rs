pub(crate) fn prefix_to_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.into();
    }
    let mut end = max_bytes;
    while end > 0 && !text.is_char_boundary(end) {
        end -= 1;
    }
    text[..end].into()
}

pub(crate) fn suffix_to_bytes(text: &str, max_bytes: usize) -> String {
    if text.len() <= max_bytes {
        return text.into();
    }
    let mut start = text.len().saturating_sub(max_bytes);
    while start < text.len() && !text.is_char_boundary(start) {
        start += 1;
    }
    text[start..].into()
}

#[cfg(test)]
mod tests {
    use super::{prefix_to_bytes, suffix_to_bytes};

    #[test]
    fn byte_limited_text_preserves_utf8_boundaries() {
        let text = "a你好b";

        assert_eq!(prefix_to_bytes(text, 3), "a");
        assert_eq!(suffix_to_bytes(text, 3), "b");
    }
}
