pub const DEFAULT_TELEGRAM_TEXT_LIMIT_UTF16_UNITS: usize = 3_900;

pub fn telegram_utf16_units(text: &str) -> usize {
    text.encode_utf16().count()
}

pub(crate) fn truncate_end_chars(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }
    if text.chars().count() <= max_chars {
        return text.into();
    }
    if max_chars <= 3 {
        return ".".repeat(max_chars);
    }
    let end = char_boundary_after(text, max_chars - 3);
    format!("{}...", &text[..end])
}

pub(crate) fn truncate_middle_chars(text: &str, max_chars: usize) -> String {
    let char_count = text.chars().count();
    if char_count <= max_chars {
        return text.into();
    }
    if max_chars <= 5 {
        return truncate_end_chars(text, max_chars);
    }

    let separator = " ... ";
    let keep = max_chars.saturating_sub(separator.chars().count());
    let head = keep / 2;
    let tail = keep.saturating_sub(head);
    let head_end = char_boundary_after(text, head);
    let tail_start = char_boundary_from_end(text, tail);

    format!("{}{}{}", &text[..head_end], separator, &text[tail_start..])
}

pub(crate) fn truncate_string_to_chars(target: &mut String, max_chars: usize) {
    let Some((index, _)) = target.char_indices().nth(max_chars) else {
        return;
    };
    target.truncate(index);
}

pub(crate) fn whitespace_prefix_summary(text: &str, max_chars: usize) -> String {
    if max_chars == 0 {
        return String::new();
    }

    let mut summary = String::new();
    let mut chars = 0;
    let mut truncated = false;
    for word in text.split_whitespace() {
        let separator_chars = usize::from(!summary.is_empty());
        let word_chars = word.chars().count();
        if chars + separator_chars + word_chars <= max_chars {
            if separator_chars == 1 {
                summary.push(' ');
                chars += 1;
            }
            summary.push_str(word);
            chars += word_chars;
            continue;
        }

        truncated = true;
        if separator_chars == 1 && chars < max_chars {
            summary.push(' ');
            chars += 1;
        }
        for ch in word.chars().take(max_chars.saturating_sub(chars)) {
            summary.push(ch);
        }
        break;
    }

    if truncated {
        summary.push_str("...");
    }
    summary
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

fn char_boundary_after(text: &str, chars: usize) -> usize {
    if chars == 0 {
        return 0;
    }
    text.char_indices()
        .nth(chars)
        .map(|(index, _)| index)
        .unwrap_or(text.len())
}

fn char_boundary_from_end(text: &str, chars: usize) -> usize {
    if chars == 0 {
        return text.len();
    }
    text.char_indices()
        .rev()
        .nth(chars.saturating_sub(1))
        .map(|(index, _)| index)
        .unwrap_or(0)
}

fn continuation_marker(total: usize) -> String {
    format!("[{total}/{total}]\n")
}

#[cfg(test)]
mod tests {
    use super::{
        split_telegram_text, split_telegram_text_with_continuation, telegram_utf16_units,
        truncate_middle_chars, whitespace_prefix_summary,
    };

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

    #[test]
    fn truncate_middle_uses_char_boundaries() {
        assert_eq!(truncate_middle_chars("a你b好cdef", 7), "a ... f");
    }

    #[test]
    fn whitespace_prefix_summary_collapses_and_truncates() {
        assert_eq!(
            whitespace_prefix_summary("  alpha\n beta gamma", 12),
            "alpha beta g..."
        );
    }
}
