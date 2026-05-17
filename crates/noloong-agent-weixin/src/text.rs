pub const DEFAULT_WEIXIN_TEXT_LIMIT_CHARS: usize = 3500;
const COPY_FRIENDLY_LINE_WIDTH: usize = 120;

pub fn weixin_char_count(text: &str) -> usize {
    text.chars().count()
}

pub fn normalize_weixin_markdown(text: &str) -> String {
    let mut result = Vec::new();
    let mut in_code_block = false;
    let mut blank_run = 0;
    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        if is_fence(line.trim()) {
            in_code_block = !in_code_block;
            result.push(line.to_owned());
            blank_run = 0;
            continue;
        }
        if in_code_block {
            result.push(line.to_owned());
            continue;
        }
        if line.trim().is_empty() {
            blank_run += 1;
            if blank_run <= 1 {
                result.push(String::new());
            }
            continue;
        }
        blank_run = 0;
        result.push(rewrite_heading(line));
    }
    rewrite_pipe_tables(&wrap_long_lines(&result.join("\n")))
        .trim()
        .to_owned()
}

pub fn split_weixin_text(text: &str, max_chars: usize) -> Vec<String> {
    let text = text.trim();
    if text.is_empty() {
        return Vec::new();
    }
    let max_chars = max_chars.max(1);
    if weixin_char_count(text) <= max_chars {
        return vec![text.into()];
    }

    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0;
    let mut in_code_block = false;
    for block in markdown_blocks(text) {
        let block_chars = weixin_char_count(&block);
        if current_chars + separator_chars(&current) + block_chars <= max_chars {
            append_block(&mut current, &mut current_chars, &block);
            if is_fence(block.trim()) {
                in_code_block = !in_code_block;
            }
            continue;
        }
        if !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            current_chars = 0;
        }
        if block_chars <= max_chars {
            append_block(&mut current, &mut current_chars, &block);
            continue;
        }
        split_long_block(&block, max_chars, &mut chunks, &mut in_code_block);
    }
    if !current.trim().is_empty() {
        chunks.push(current);
    }
    if chunks.len() <= 1 {
        return chunks;
    }
    let total = chunks.len();
    chunks
        .into_iter()
        .enumerate()
        .map(|(index, chunk)| format!("[{}/{}]\n{chunk}", index + 1, total))
        .collect()
}

fn markdown_blocks(text: &str) -> Vec<String> {
    let mut blocks = Vec::new();
    let mut current = Vec::new();
    let mut in_code_block = false;
    for line in text.lines() {
        let trimmed = line.trim();
        if is_fence(trimmed) {
            if !in_code_block && !current.is_empty() {
                blocks.push(current.join("\n"));
                current.clear();
            }
            current.push(line.to_owned());
            in_code_block = !in_code_block;
            if !in_code_block {
                blocks.push(current.join("\n"));
                current.clear();
            }
            continue;
        }
        if in_code_block {
            current.push(line.to_owned());
            continue;
        }
        if trimmed.is_empty() {
            if !current.is_empty() {
                blocks.push(current.join("\n"));
                current.clear();
            }
            continue;
        }
        current.push(line.to_owned());
    }
    if !current.is_empty() {
        blocks.push(current.join("\n"));
    }
    blocks
}

fn split_long_block(
    block: &str,
    max_chars: usize,
    chunks: &mut Vec<String>,
    _in_code_block: &mut bool,
) {
    let mut current = String::new();
    let mut chars = 0;
    for ch in block.chars() {
        let ch_len = 1;
        if chars + ch_len > max_chars && !current.is_empty() {
            chunks.push(std::mem::take(&mut current));
            chars = 0;
        }
        current.push(ch);
        chars += ch_len;
    }
    if !current.is_empty() {
        chunks.push(current);
    }
}

fn append_block(current: &mut String, current_chars: &mut usize, block: &str) {
    let separator = separator_chars(current);
    if separator > 0 {
        current.push_str("\n\n");
        *current_chars += separator;
    }
    current.push_str(block);
    *current_chars += weixin_char_count(block);
}

fn separator_chars(current: &str) -> usize {
    if current.is_empty() { 0 } else { 2 }
}

fn rewrite_heading(line: &str) -> String {
    let trimmed = line.trim();
    let hashes = trimmed.chars().take_while(|ch| *ch == '#').count();
    if (1..=6).contains(&hashes) && trimmed.chars().nth(hashes) == Some(' ') {
        let title = trimmed[hashes + 1..].trim();
        if hashes == 1 {
            return format!("〖{title}〗");
        }
        return format!("**{title}**");
    }
    line.to_owned()
}

fn wrap_long_lines(text: &str) -> String {
    let mut wrapped = Vec::new();
    let mut in_code_block = false;
    for raw_line in text.lines() {
        let line = raw_line.trim_end();
        if is_fence(line.trim()) {
            in_code_block = !in_code_block;
            wrapped.push(line.to_owned());
            continue;
        }
        if in_code_block || line.chars().count() <= COPY_FRIENDLY_LINE_WIDTH {
            wrapped.push(line.to_owned());
            continue;
        }
        wrapped.extend(wrap_line(line, COPY_FRIENDLY_LINE_WIDTH));
    }
    wrapped.join("\n")
}

fn wrap_line(line: &str, width: usize) -> Vec<String> {
    let mut output = Vec::new();
    let mut current = String::new();
    for word in line.split_whitespace() {
        let next_len =
            current.chars().count() + usize::from(!current.is_empty()) + word.chars().count();
        if next_len > width && !current.is_empty() {
            output.push(std::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        output.push(current);
    }
    if output.is_empty() {
        output.push(line.to_owned());
    }
    output
}

fn rewrite_pipe_tables(text: &str) -> String {
    let lines = text.lines().collect::<Vec<_>>();
    let mut output = Vec::new();
    let mut index = 0;
    while index < lines.len() {
        if index + 1 < lines.len() && is_pipe_row(lines[index]) && is_separator(lines[index + 1]) {
            let headers = split_pipe_row(lines[index]);
            index += 2;
            while index < lines.len() && is_pipe_row(lines[index]) {
                let cells = split_pipe_row(lines[index]);
                let pairs = headers
                    .iter()
                    .zip(cells.iter())
                    .map(|(header, cell)| format!("{header}: {cell}"))
                    .collect::<Vec<_>>();
                output.push(format!("- {}", pairs.join(" | ")));
                index += 1;
            }
        } else {
            output.push(lines[index].to_owned());
            index += 1;
        }
    }
    output.join("\n")
}

fn is_pipe_row(line: &str) -> bool {
    let trimmed = line.trim();
    trimmed.starts_with('|') && trimmed.ends_with('|') && trimmed.matches('|').count() >= 2
}

fn is_separator(line: &str) -> bool {
    split_pipe_row(line)
        .into_iter()
        .all(|cell| cell.chars().all(|ch| matches!(ch, '-' | ':' | ' ')))
}

fn split_pipe_row(line: &str) -> Vec<String> {
    line.trim()
        .trim_matches('|')
        .split('|')
        .map(str::trim)
        .filter(|cell| !cell.is_empty())
        .map(str::to_owned)
        .collect()
}

fn is_fence(line: &str) -> bool {
    line.starts_with("```")
}

#[cfg(test)]
mod tests {
    use super::{normalize_weixin_markdown, split_weixin_text};

    #[test]
    fn normalizes_headings_and_tables() {
        let text = normalize_weixin_markdown("# Title\n\n| A | B |\n|---|---|\n| x | y |");

        assert!(text.contains("〖Title〗"));
        assert!(text.contains("- A: x | B: y"));
    }

    #[test]
    fn split_preserves_small_messages() {
        assert_eq!(split_weixin_text("hello", 10), vec!["hello"]);
    }

    #[test]
    fn split_marks_multiple_chunks() {
        let chunks = split_weixin_text("abcdefghij", 4);

        assert_eq!(chunks.len(), 3);
        assert!(chunks[0].starts_with("[1/3]\n"));
    }
}
