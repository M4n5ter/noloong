const FADE_IN_MS: u64 = 150;
const FRESH_OPACITY: f32 = 0.35;

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct StreamingText {
    segments: Vec<StreamingSegment>,
}

impl StreamingText {
    pub fn push_delta(&mut self, text: impl Into<String>, at_ms: u64) {
        let text = text.into();
        if text.is_empty() {
            return;
        }
        self.segments.push(StreamingSegment { text, at_ms });
    }

    pub fn text(&self) -> String {
        self.segments
            .iter()
            .map(|segment| segment.text.as_str())
            .collect()
    }

    pub fn visible_segments(&self, now_ms: u64) -> Vec<RenderedStreamingSegment> {
        self.segments
            .iter()
            .map(|segment| RenderedStreamingSegment {
                text: segment.text.clone(),
                opacity: opacity_at(segment.at_ms, now_ms),
            })
            .collect()
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct StreamingSegment {
    text: String,
    at_ms: u64,
}

#[derive(Clone, Debug, PartialEq)]
pub struct RenderedStreamingSegment {
    pub text: String,
    pub opacity: f32,
}

fn opacity_at(start_ms: u64, now_ms: u64) -> f32 {
    let elapsed = now_ms.saturating_sub(start_ms).min(FADE_IN_MS);
    let progress = elapsed as f32 / FADE_IN_MS as f32;
    round_2(FRESH_OPACITY + (1.0 - FRESH_OPACITY) * progress)
}

fn round_2(value: f32) -> f32 {
    (value * 100.0).round() / 100.0
}
