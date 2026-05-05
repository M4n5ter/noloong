use crate::provider_utils::emit_model_stream_event;
use crate::{AgentCoreError, CancellationToken, ModelStreamEvent, ModelStreamSink, Result};
use bytes::{Buf, BytesMut};
use memchr::memchr2;
use reqwest::{RequestBuilder, StatusCode};
use std::str;
use std::time::Duration;

const HTTP_ERROR_BODY_PREVIEW_LIMIT: usize = 2048;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SseReconnectConfig {
    pub max_reconnects: usize,
    pub initial_backoff: Duration,
    pub max_backoff: Duration,
}

impl SseReconnectConfig {
    pub fn disabled() -> Self {
        Self {
            max_reconnects: 0,
            initial_backoff: Duration::from_millis(0),
            max_backoff: Duration::from_millis(0),
        }
    }
}

impl Default for SseReconnectConfig {
    fn default() -> Self {
        Self {
            max_reconnects: 2,
            initial_backoff: Duration::from_millis(200),
            max_backoff: Duration::from_secs(2),
        }
    }
}

pub(crate) struct SseStreamOptions<'a> {
    pub(crate) provider_label: &'a str,
    pub(crate) request_timeout: Duration,
    pub(crate) stream_idle_timeout: Duration,
    pub(crate) reconnect: &'a SseReconnectConfig,
    pub(crate) cancellation: &'a CancellationToken,
}

pub(crate) struct SseFrameResult {
    pub(crate) events: Vec<ModelStreamEvent>,
    pub(crate) terminal: bool,
}

impl SseFrameResult {
    pub(crate) fn new(events: Vec<ModelStreamEvent>, terminal: bool) -> Self {
        Self { events, terminal }
    }
}

pub(crate) async fn run_sse_model_stream(
    options: SseStreamOptions<'_>,
    stream: &ModelStreamSink,
    events: &mut Vec<ModelStreamEvent>,
    mut build_request: impl FnMut() -> Result<RequestBuilder>,
    mut handle_data: impl FnMut(&str) -> Result<SseFrameResult>,
) -> Result<()> {
    let mut reconnects = 0;
    loop {
        match run_sse_attempt(
            &options,
            stream,
            events,
            &mut build_request,
            &mut handle_data,
        )
        .await?
        {
            SseAttemptOutcome::Terminal => return Ok(()),
            SseAttemptOutcome::RetryableBeforeData(message) => {
                if reconnects >= options.reconnect.max_reconnects {
                    return Err(AgentCoreError::Provider(format!(
                        "{} stream failed after {} reconnect attempt(s): {message}",
                        options.provider_label, reconnects
                    )));
                }
                sleep_before_reconnect(options.cancellation, options.reconnect, reconnects).await?;
                reconnects += 1;
            }
        }
    }
}

enum SseAttemptOutcome {
    Terminal,
    RetryableBeforeData(String),
}

async fn run_sse_attempt(
    options: &SseStreamOptions<'_>,
    stream: &ModelStreamSink,
    events: &mut Vec<ModelStreamEvent>,
    build_request: &mut impl FnMut() -> Result<RequestBuilder>,
    handle_data: &mut impl FnMut(&str) -> Result<SseFrameResult>,
) -> Result<SseAttemptOutcome> {
    options.cancellation.throw_if_cancelled()?;
    let request = build_request()?;
    let request_timeout = tokio::time::sleep(options.request_timeout);
    tokio::pin!(request_timeout);
    let response = tokio::select! {
        response = request.send() => match response {
            Ok(response) => response,
            Err(error) => {
                return Ok(SseAttemptOutcome::RetryableBeforeData(format!(
                    "{} request failed before SSE data: {error}",
                    options.provider_label
                )));
            }
        },
        _ = options.cancellation.cancelled() => return Err(AgentCoreError::Aborted),
        _ = &mut request_timeout => {
            return Err(AgentCoreError::Provider(format!(
                "{} request timed out",
                options.provider_label
            )));
        }
    };

    if !response.status().is_success() {
        return handle_http_error(options, response).await;
    }

    read_sse_response(options, stream, events, response, handle_data).await
}

async fn handle_http_error(
    options: &SseStreamOptions<'_>,
    response: reqwest::Response,
) -> Result<SseAttemptOutcome> {
    let status = response.status();
    let body = read_error_body_preview(options, response).await?;
    let message = format!(
        "{} request failed with status {status}: {}",
        options.provider_label, body
    );
    if is_retryable_status(status) {
        Ok(SseAttemptOutcome::RetryableBeforeData(message))
    } else {
        Err(AgentCoreError::HttpStatus {
            provider: options.provider_label.into(),
            status: status.as_u16(),
            body,
        })
    }
}

async fn read_error_body_preview(
    options: &SseStreamOptions<'_>,
    mut response: reqwest::Response,
) -> Result<String> {
    let mut body = Vec::new();
    while body.len() < HTTP_ERROR_BODY_PREVIEW_LIMIT {
        let chunk = tokio::select! {
            chunk = response.chunk() => chunk.unwrap_or(None),
            _ = options.cancellation.cancelled() => return Err(AgentCoreError::Aborted),
            _ = tokio::time::sleep(options.stream_idle_timeout) => None,
        };
        let Some(chunk) = chunk else {
            break;
        };
        let remaining = HTTP_ERROR_BODY_PREVIEW_LIMIT - body.len();
        body.extend_from_slice(&chunk[..chunk.len().min(remaining)]);
    }
    Ok(String::from_utf8_lossy(&body).into_owned())
}

async fn read_sse_response(
    options: &SseStreamOptions<'_>,
    stream: &ModelStreamSink,
    events: &mut Vec<ModelStreamEvent>,
    mut response: reqwest::Response,
    handle_data: &mut impl FnMut(&str) -> Result<SseFrameResult>,
) -> Result<SseAttemptOutcome> {
    let mut decoder = SseDecoder::default();
    let mut data_delivered = false;
    let mut terminal = false;
    let mut frame_results = Vec::new();

    loop {
        let chunk = tokio::select! {
            chunk = response.chunk() => match chunk {
                Ok(chunk) => chunk,
                Err(error) => {
                    return broken_stream(data_delivered, format!(
                        "{} stream chunk failed: {error}",
                        options.provider_label
                    ));
                }
            },
            _ = options.cancellation.cancelled() => return Err(AgentCoreError::Aborted),
            _ = tokio::time::sleep(options.stream_idle_timeout) => {
                return broken_stream(data_delivered, format!(
                    "{} stream timed out",
                    options.provider_label
                ));
            }
        };

        let Some(chunk) = chunk else {
            drain_decoder_finish(
                &mut decoder,
                &mut data_delivered,
                &mut terminal,
                &mut frame_results,
                handle_data,
            )?;
            emit_frame_results(stream, events, &mut frame_results).await?;
            if terminal {
                return Ok(SseAttemptOutcome::Terminal);
            }
            return broken_stream(
                data_delivered,
                format!(
                    "{} stream ended before terminal event",
                    options.provider_label
                ),
            );
        };

        drain_decoder_chunk(
            &mut decoder,
            &chunk,
            &mut data_delivered,
            &mut terminal,
            &mut frame_results,
            handle_data,
        )?;
        emit_frame_results(stream, events, &mut frame_results).await?;
        if terminal {
            return Ok(SseAttemptOutcome::Terminal);
        }
    }
}

fn drain_decoder_chunk(
    decoder: &mut SseDecoder,
    chunk: &[u8],
    data_delivered: &mut bool,
    terminal: &mut bool,
    frame_results: &mut Vec<SseFrameResult>,
    handle_data: &mut impl FnMut(&str) -> Result<SseFrameResult>,
) -> Result<()> {
    decoder.push(chunk, |data| {
        handle_decoded_data(data, data_delivered, terminal, frame_results, handle_data)
    })?;
    Ok(())
}

fn drain_decoder_finish(
    decoder: &mut SseDecoder,
    data_delivered: &mut bool,
    terminal: &mut bool,
    frame_results: &mut Vec<SseFrameResult>,
    handle_data: &mut impl FnMut(&str) -> Result<SseFrameResult>,
) -> Result<()> {
    decoder.finish(|data| {
        handle_decoded_data(data, data_delivered, terminal, frame_results, handle_data)
    })?;
    Ok(())
}

fn handle_decoded_data(
    data: &str,
    data_delivered: &mut bool,
    terminal: &mut bool,
    frame_results: &mut Vec<SseFrameResult>,
    handle_data: &mut impl FnMut(&str) -> Result<SseFrameResult>,
) -> Result<SseDecodeControl> {
    *data_delivered = true;
    let result = handle_data(data)?;
    let control = if result.terminal {
        *terminal = true;
        SseDecodeControl::Stop
    } else {
        SseDecodeControl::Continue
    };
    frame_results.push(result);
    Ok(control)
}

async fn emit_frame_results(
    stream: &ModelStreamSink,
    events: &mut Vec<ModelStreamEvent>,
    frame_results: &mut Vec<SseFrameResult>,
) -> Result<()> {
    for frame_result in frame_results.drain(..) {
        for event in frame_result.events {
            emit_model_stream_event(stream, events, event).await?;
        }
    }
    Ok(())
}

fn broken_stream(data_delivered: bool, message: String) -> Result<SseAttemptOutcome> {
    if data_delivered {
        Err(AgentCoreError::Provider(message))
    } else {
        Ok(SseAttemptOutcome::RetryableBeforeData(message))
    }
}

async fn sleep_before_reconnect(
    cancellation: &CancellationToken,
    config: &SseReconnectConfig,
    reconnects: usize,
) -> Result<()> {
    let delay = reconnect_delay(config, reconnects);
    if delay.is_zero() {
        return Ok(());
    }
    tokio::select! {
        _ = tokio::time::sleep(delay) => Ok(()),
        _ = cancellation.cancelled() => Err(AgentCoreError::Aborted),
    }
}

fn reconnect_delay(config: &SseReconnectConfig, reconnects: usize) -> Duration {
    let multiplier = 2_u32.saturating_pow(reconnects.min(31) as u32);
    config
        .initial_backoff
        .checked_mul(multiplier)
        .unwrap_or(config.max_backoff)
        .min(config.max_backoff)
}

fn is_retryable_status(status: StatusCode) -> bool {
    status == StatusCode::REQUEST_TIMEOUT
        || status == StatusCode::TOO_MANY_REQUESTS
        || status.is_server_error()
}

#[derive(Default)]
pub(crate) struct SseDecoder {
    buffer: BytesMut,
    scratch: Vec<u8>,
}

pub(crate) enum SseDecodeControl {
    Continue,
    Stop,
}

impl SseDecoder {
    pub(crate) fn push(
        &mut self,
        chunk: impl AsRef<[u8]>,
        mut on_data: impl FnMut(&str) -> Result<SseDecodeControl>,
    ) -> Result<()> {
        self.buffer.extend_from_slice(chunk.as_ref());
        self.drain_frames(&mut on_data)
    }

    pub(crate) fn finish(
        &mut self,
        mut on_data: impl FnMut(&str) -> Result<SseDecodeControl>,
    ) -> Result<()> {
        if self.buffer.iter().any(|byte| !byte.is_ascii_whitespace()) {
            self.buffer.extend_from_slice(b"\n\n");
        }
        self.drain_frames(&mut on_data)
    }

    fn drain_frames(
        &mut self,
        on_data: &mut impl FnMut(&str) -> Result<SseDecodeControl>,
    ) -> Result<()> {
        while let Some((frame_end, delimiter_end)) = find_frame_boundary(&self.buffer) {
            let frame = &self.buffer[..frame_end];
            let control = emit_frame_data(frame, &mut self.scratch, on_data)?;
            self.buffer.advance(delimiter_end);
            if matches!(control, SseDecodeControl::Stop) {
                self.buffer.clear();
                break;
            }
        }
        Ok(())
    }
}

fn find_frame_boundary(buffer: &[u8]) -> Option<(usize, usize)> {
    let mut line_start = 0;
    while let Some((line_end, next_line_start)) = find_line_boundary(buffer, line_start) {
        if line_end == line_start {
            return Some((line_start, next_line_start));
        }
        line_start = next_line_start;
    }
    None
}

fn find_line_boundary(buffer: &[u8], start: usize) -> Option<(usize, usize)> {
    let offset = memchr2(b'\n', b'\r', &buffer[start..])?;
    let line_end = start + offset;
    let next_line_start = match buffer[line_end] {
        b'\r' if line_end + 1 == buffer.len() => return None,
        b'\r' if buffer[line_end + 1] == b'\n' => line_end + 2,
        _ => line_end + 1,
    };
    Some((line_end, next_line_start))
}

fn emit_frame_data(
    frame: &[u8],
    scratch: &mut Vec<u8>,
    on_data: &mut impl FnMut(&str) -> Result<SseDecodeControl>,
) -> Result<SseDecodeControl> {
    let mut single_line = None;
    let mut data_line_count = 0;
    let mut line_start = 0;
    scratch.clear();

    while line_start < frame.len() {
        let (line, next_line_start) = match find_line_boundary(frame, line_start) {
            Some((line_end, next_line_start)) => (&frame[line_start..line_end], next_line_start),
            None => (&frame[line_start..], frame.len()),
        };
        line_start = next_line_start;

        let Some(value) = data_field_value(line) else {
            continue;
        };
        if data_line_count == 0 {
            single_line = Some(value);
        } else {
            if data_line_count == 1 {
                scratch.extend_from_slice(single_line.take().unwrap_or_default());
            }
            scratch.push(b'\n');
            scratch.extend_from_slice(value);
        }
        data_line_count += 1;
    }

    match data_line_count {
        0 => Ok(SseDecodeControl::Continue),
        1 => {
            let data = single_line.unwrap_or_default();
            if data.is_empty() {
                return Ok(SseDecodeControl::Continue);
            }
            on_data(data_to_str(data)?)
        }
        _ => {
            if scratch.is_empty() {
                return Ok(SseDecodeControl::Continue);
            }
            on_data(data_to_str(scratch)?)
        }
    }
}

fn data_field_value(line: &[u8]) -> Option<&[u8]> {
    if line == b"data" {
        return Some(b"");
    }
    let value = line.strip_prefix(b"data:")?;
    Some(value.strip_prefix(b" ").unwrap_or(value))
}

fn data_to_str(data: &[u8]) -> Result<&str> {
    str::from_utf8(data)
        .map_err(|error| AgentCoreError::Provider(format!("invalid utf-8 in SSE data: {error}")))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(decoder: &mut SseDecoder, chunk: impl AsRef<[u8]>) -> Result<Vec<String>> {
        let mut frames = Vec::new();
        decoder.push(chunk, |data| {
            frames.push(data.to_string());
            Ok(SseDecodeControl::Continue)
        })?;
        Ok(frames)
    }

    fn finish(decoder: &mut SseDecoder) -> Result<Vec<String>> {
        let mut frames = Vec::new();
        decoder.finish(|data| {
            frames.push(data.to_string());
            Ok(SseDecodeControl::Continue)
        })?;
        Ok(frames)
    }

    #[test]
    fn sse_decoder_handles_multiline_data_and_done() {
        let mut decoder = SseDecoder::default();

        let frames = collect(
            &mut decoder,
            ": ignored\n\ndata: one\ndata: two\n\ndata: [DONE]\n\n",
        )
        .expect("decode frames");

        assert_eq!(frames, ["one\ntwo", "[DONE]"]);
    }

    #[test]
    fn sse_decoder_normalizes_split_crlf_without_extra_frame_boundary() {
        let mut decoder = SseDecoder::default();

        assert!(collect(&mut decoder, "data: one\r").unwrap().is_empty());
        assert!(collect(&mut decoder, "\ndata: two\r").unwrap().is_empty());
        assert_eq!(collect(&mut decoder, "\n\r\n").unwrap(), ["one\ntwo"]);
    }

    #[test]
    fn sse_decoder_preserves_utf8_split_across_chunks() {
        let mut decoder = SseDecoder::default();

        assert!(
            collect(&mut decoder, [b'd', b'a', b't', b'a', b':', b' ', 0xE4])
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            collect(&mut decoder, [0xBD, 0xA0, b'\n', b'\n']).unwrap(),
            ["你"]
        );
    }

    #[test]
    fn sse_decoder_stops_after_callback_stop() {
        let mut decoder = SseDecoder::default();
        let mut frames = Vec::new();

        decoder
            .push("data: one\n\ndata: two\n\n", |data| {
                frames.push(data.to_string());
                Ok(SseDecodeControl::Stop)
            })
            .expect("decode frames");

        assert_eq!(frames, ["one"]);
        assert!(finish(&mut decoder).unwrap().is_empty());
    }

    #[test]
    fn sse_decoder_flushes_partial_data_on_finish() {
        let mut decoder = SseDecoder::default();

        assert!(collect(&mut decoder, "data: one").unwrap().is_empty());

        assert_eq!(finish(&mut decoder).unwrap(), ["one"]);
    }

    #[test]
    fn sse_decoder_ignores_comment_only_finish() {
        let mut decoder = SseDecoder::default();

        assert!(collect(&mut decoder, ": ignored").unwrap().is_empty());

        assert!(finish(&mut decoder).unwrap().is_empty());
    }

    #[test]
    fn sse_decoder_rejects_invalid_utf8() {
        let mut decoder = SseDecoder::default();

        let error = collect(&mut decoder, b"data: \xFF\n\n").expect_err("invalid utf-8");

        assert!(error.to_string().contains("invalid utf-8 in SSE data"));
    }
}
