pub const DEFAULT_TELEGRAM_TEXT_LIMIT_UTF16_UNITS: usize = 3_900;

pub fn telegram_utf16_units(text: &str) -> usize {
    text.encode_utf16().count()
}

pub fn split_telegram_text(text: &str, max_units: usize) -> Vec<String> {
    if text.is_empty() {
        return Vec::new();
    }

    let max_units = max_units.max(1);
    let mut messages = Vec::new();
    let mut current = String::new();
    let mut current_units = 0;
    for line in text.split_inclusive('\n') {
        let line_units = telegram_utf16_units(line);
        if current_units + line_units > max_units && !current.is_empty() {
            messages.push(std::mem::take(&mut current));
            current_units = 0;
        }
        push_line_with_limit(
            line,
            line_units,
            max_units,
            &mut current,
            &mut current_units,
            &mut messages,
        );
    }
    if !current.is_empty() {
        messages.push(current);
    }
    messages
}

pub fn split_telegram_text_with_continuation(text: &str, max_units: usize) -> Vec<String> {
    let max_units = max_units.max(1);
    let mut body_limit = max_units;
    let mut chunks = split_telegram_text(text, body_limit);
    if chunks.len() <= 1 {
        return chunks;
    }

    for _ in 0..8 {
        let marker = continuation_marker(chunks.len());
        let next_body_limit = max_units
            .saturating_sub(telegram_utf16_units(&marker))
            .max(1);
        if next_body_limit == body_limit {
            break;
        }
        body_limit = next_body_limit;
        chunks = split_telegram_text(text, body_limit);
    }

    let total = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| format!("[{}/{}]\n{chunk}", index + 1, total))
        .collect()
}

fn push_line_with_limit(
    line: &str,
    line_units: usize,
    max_units: usize,
    current: &mut String,
    current_units: &mut usize,
    messages: &mut Vec<String>,
) {
    if line_units <= max_units {
        current.push_str(line);
        *current_units += line_units;
        return;
    }

    for ch in line.chars() {
        let ch_units = ch.len_utf16();
        if *current_units + ch_units > max_units && !current.is_empty() {
            messages.push(std::mem::take(current));
            *current_units = 0;
        }
        current.push(ch);
        *current_units += ch_units;
    }
}

fn continuation_marker(total: usize) -> String {
    format!("[{total}/{total}]\n")
}

#[cfg(test)]
mod tests {
    use super::{split_telegram_text, split_telegram_text_with_continuation, telegram_utf16_units};

    #[test]
    fn split_keeps_head_and_tail_chunks() {
        let chunks = split_telegram_text("abc\ndef", 4);
        assert_eq!(chunks, vec!["abc\n", "def"]);
    }

    #[test]
    fn split_long_line_on_char_boundary() {
        let chunks = split_telegram_text("a你b好c", 3);
        assert_eq!(chunks, vec!["a你b", "好c"]);
    }

    #[test]
    fn split_uses_utf16_limit() {
        let chunks = split_telegram_text("a😀b", 3);
        assert_eq!(chunks, vec!["a😀", "b"]);
    }

    #[test]
    fn split_with_continuation_marks_all_chunks() {
        let chunks = split_telegram_text_with_continuation("abcdefghij", 8);

        assert_eq!(
            chunks,
            vec![
                "[1/5]\nab",
                "[2/5]\ncd",
                "[3/5]\nef",
                "[4/5]\ngh",
                "[5/5]\nij"
            ]
        );
        assert!(chunks.iter().all(|chunk| telegram_utf16_units(chunk) <= 8));
    }

    #[test]
    fn split_with_continuation_keeps_single_chunk_unmarked() {
        let chunks = split_telegram_text_with_continuation("abc", 9);

        assert_eq!(chunks, vec!["abc"]);
    }
}
