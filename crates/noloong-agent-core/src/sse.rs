#[derive(Default)]
pub(crate) struct SseDecoder {
    buffer: Vec<u8>,
    pending_cr: bool,
}

impl SseDecoder {
    pub(crate) fn push(&mut self, chunk: impl AsRef<[u8]>) -> Vec<String> {
        self.push_normalized(chunk);
        self.drain_frames()
    }

    pub(crate) fn finish(&mut self) -> Vec<String> {
        if self.buffer.iter().any(|byte| !byte.is_ascii_whitespace()) {
            self.buffer.extend_from_slice(b"\n\n");
        }
        self.drain_frames()
    }

    fn push_normalized(&mut self, chunk: impl AsRef<[u8]>) {
        for byte in chunk.as_ref() {
            match *byte {
                b'\r' => {
                    self.buffer.push(b'\n');
                    self.pending_cr = true;
                }
                b'\n' if self.pending_cr => {
                    self.pending_cr = false;
                }
                b'\n' => {
                    self.buffer.push(b'\n');
                    self.pending_cr = false;
                }
                byte => {
                    self.buffer.push(byte);
                    self.pending_cr = false;
                }
            }
        }
    }

    fn drain_frames(&mut self) -> Vec<String> {
        let mut frames = Vec::new();
        let mut frame_start = 0;
        let mut index = 0;
        while index + 1 < self.buffer.len() {
            if self.buffer[index] != b'\n' || self.buffer[index + 1] != b'\n' {
                index += 1;
                continue;
            }
            if let Some(data) = parse_sse_frame(&self.buffer[frame_start..index]) {
                frames.push(data);
            }
            index += 2;
            frame_start = index;
        }
        if frame_start > 0 {
            self.buffer.drain(..frame_start);
        }
        frames
    }
}

fn parse_sse_frame(frame: &[u8]) -> Option<String> {
    let mut data = Vec::new();
    for line in frame.split(|byte| *byte == b'\n') {
        let Some(line) = line.strip_prefix(b"data:") else {
            continue;
        };
        let line = line.strip_prefix(b" ").unwrap_or(line);
        if !data.is_empty() {
            data.push(b'\n');
        }
        data.extend_from_slice(line);
    }
    if data.is_empty() {
        None
    } else {
        Some(String::from_utf8_lossy(&data).into_owned())
    }
}
