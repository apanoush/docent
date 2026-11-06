//! Incremental Server-Sent Events parsing.

use std::io::Read;

use crate::error::DocentError;

/// Decodes SSE chunks into complete `data:` payloads.
#[derive(Debug, Default)]
pub(crate) struct SseDecoder {
    buffer: Vec<u8>,
    data_lines: Vec<String>,
}

impl SseDecoder {
    /// Feeds a chunk and dispatches every completed event.
    pub(crate) fn push(
        &mut self,
        chunk: &[u8],
        on_event: &mut dyn FnMut(&str) -> Result<(), DocentError>,
    ) -> Result<(), DocentError> {
        self.buffer.extend_from_slice(chunk);

        while let Some(index) = self.buffer.iter().position(|byte| *byte == b'\n') {
            let line: Vec<u8> = self.buffer.drain(..=index).collect();
            let line = String::from_utf8_lossy(&line);
            let line = line.trim_end_matches(['\r', '\n']);

            if line.is_empty() {
                self.flush_event(on_event)?;
            } else if let Some(data) = line.strip_prefix("data:") {
                self.data_lines.push(data.trim_start().to_string());
            }
            // `event:`, `id:`, `retry:`, and comment lines are ignored. Both
            // providers repeat the event type inside the JSON payload.
        }

        Ok(())
    }

    /// Flushes a final line that arrived without a trailing newline.
    pub(crate) fn finish(
        &mut self,
        on_event: &mut dyn FnMut(&str) -> Result<(), DocentError>,
    ) -> Result<(), DocentError> {
        if !self.buffer.is_empty() {
            let line = String::from_utf8_lossy(&self.buffer)
                .trim_end_matches(['\r', '\n'])
                .to_string();
            self.buffer.clear();
            if let Some(data) = line.strip_prefix("data:") {
                self.data_lines.push(data.trim_start().to_string());
            }
        }
        self.flush_event(on_event)
    }

    fn flush_event(
        &mut self,
        on_event: &mut dyn FnMut(&str) -> Result<(), DocentError>,
    ) -> Result<(), DocentError> {
        if self.data_lines.is_empty() {
            return Ok(());
        }
        let data = self.data_lines.join("\n");
        self.data_lines.clear();
        on_event(&data)
    }
}

/// Reads an SSE stream until EOF.
pub(crate) fn stream_sse(
    mut reader: impl Read,
    on_event: &mut dyn FnMut(&str) -> Result<(), DocentError>,
) -> Result<(), DocentError> {
    let mut decoder = SseDecoder::default();
    let mut buffer = [0u8; 8192];

    loop {
        let read = reader
            .read(&mut buffer)
            .map_err(|error| DocentError::ProviderTransport {
                message: format!("failed to read stream: {error}"),
            })?;
        if read == 0 {
            break;
        }
        decoder.push(&buffer[..read], on_event)?;
    }

    decoder.finish(on_event)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn collect(chunks: &[&[u8]]) -> Vec<String> {
        let mut decoder = SseDecoder::default();
        let mut events = Vec::new();
        for chunk in chunks {
            decoder
                .push(chunk, &mut |data| {
                    events.push(data.to_string());
                    Ok(())
                })
                .unwrap();
        }
        decoder
            .finish(&mut |data| {
                events.push(data.to_string());
                Ok(())
            })
            .unwrap();
        events
    }

    #[test]
    fn parses_one_event() {
        let events = collect(&[b"event: message\ndata: {\"x\":1}\n\n"]);
        assert_eq!(events, vec![r#"{"x":1}"#]);
    }

    #[test]
    fn parses_multiple_events_in_one_chunk() {
        let events = collect(&[b"data: one\n\ndata: two\n\n"]);
        assert_eq!(events, vec!["one", "two"]);
    }

    #[test]
    fn handles_arbitrary_chunk_boundaries() {
        let body =
            b"event: content_block_delta\ndata: {\"text\":\"h\xc3\xa9llo\"}\n\ndata: [DONE]\n\n";
        let mut events = Vec::new();
        let mut decoder = SseDecoder::default();
        for byte in body {
            decoder
                .push(&[*byte], &mut |data| {
                    events.push(data.to_string());
                    Ok(())
                })
                .unwrap();
        }
        decoder
            .finish(&mut |data| {
                events.push(data.to_string());
                Ok(())
            })
            .unwrap();
        assert_eq!(events, vec![r#"{"text":"héllo"}"#, "[DONE]"]);
    }

    #[test]
    fn flushes_a_final_line_without_a_blank_line() {
        let events = collect(&[b"data: last"]);
        assert_eq!(events, vec!["last"]);
    }

    #[test]
    fn ignores_comments_and_blank_lines() {
        let events = collect(&[b": keep-alive\n\nevent: ping\n\n"]);
        assert!(events.is_empty());
    }
}
