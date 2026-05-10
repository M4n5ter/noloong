use noloong_agent::{JobSnapshot, ProcessOutput, ProcessOutputStream};

pub const PROCESS_OUTPUT_INLINE_CHAR_LIMIT: usize = 3_000;
const PROCESS_OUTPUT_READ_MAX_BYTES: usize = 8 * 1024;
const PROCESS_OUTPUT_WAIT_MS: u64 = 0;
const PROCESS_WAIT_TIMEOUT_MS: u64 = 1_000;

pub const fn process_output_read_max_bytes() -> usize {
    PROCESS_OUTPUT_READ_MAX_BYTES
}

pub const fn process_output_wait_ms() -> u64 {
    PROCESS_OUTPUT_WAIT_MS
}

pub const fn process_wait_timeout_ms() -> u64 {
    PROCESS_WAIT_TIMEOUT_MS
}

pub fn render_process_output(output: &ProcessOutput) -> String {
    let mut text = String::new();
    for chunk in &output.chunks {
        if !text.is_empty() {
            text.push('\n');
        }
        text.push_str(stream_label(chunk.stream));
        text.push_str(" #");
        text.push_str(&chunk.seq.to_string());
        text.push('\n');
        text.push_str(&chunk.text);
    }
    if text.trim().is_empty() {
        text.push_str("[no output]");
    }
    text
}

pub fn process_output_filename(job_id: &str) -> String {
    format!("process-{job_id}.txt")
}

pub fn process_output_document_bytes(output: &ProcessOutput) -> Vec<u8> {
    render_process_output(output).into_bytes()
}

pub fn process_snapshot_label(snapshot: &JobSnapshot) -> String {
    format!("{} {}", snapshot.job_id, snapshot.command)
}

fn stream_label(stream: ProcessOutputStream) -> &'static str {
    match stream {
        ProcessOutputStream::Stdout => "stdout",
        ProcessOutputStream::Stderr => "stderr",
    }
}

#[cfg(test)]
mod tests {
    use super::{process_output_filename, render_process_output};
    use noloong_agent::{JobStatus, OutputChunk, ProcessOutput, ProcessOutputStream};

    #[test]
    fn process_output_renders_stream_chunks() {
        let output = ProcessOutput {
            job_id: "job-1".into(),
            chunks: vec![OutputChunk {
                seq: 1,
                stream: ProcessOutputStream::Stdout,
                text: "hello".into(),
                byte_len: 5,
            }],
            next_cursor: 2,
            dropped_before_seq: 0,
            truncated: false,
            status: JobStatus::Running,
        };

        assert_eq!(render_process_output(&output), "stdout #1\nhello");
        assert_eq!(process_output_filename("job-1"), "process-job-1.txt");
    }
}
