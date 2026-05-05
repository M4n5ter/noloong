pub(crate) fn body_preview(body: &str) -> String {
    const LIMIT: usize = 2048;
    let mut chars = body.chars();
    let preview: String = chars.by_ref().take(LIMIT).collect();
    if chars.next().is_some() {
        format!("{preview}...[truncated]")
    } else {
        preview
    }
}
