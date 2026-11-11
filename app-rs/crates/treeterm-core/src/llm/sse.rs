//! Server-sent events of a streamed chat completion, decoded the way the OpenAI SDK's
//! `Stream.fromSSEResponse` does. Sans-IO: feed body chunks, get content deltas.

use serde_json::Value;

use super::{LlmError, truthy};

/// One dispatched event.
#[derive(Debug, Clone, PartialEq, Eq)]
struct SseEvent {
    event: Option<String>,
    data: String,
}

/// Splits bytes into lines (`\n`, `\r\n` or `\r`) and lines into events (`SSEDecoder`).
#[derive(Debug, Default)]
struct SseDecoder {
    buffer: Vec<u8>,
    event: Option<String>,
    data: Vec<String>,
}

impl SseDecoder {
    fn feed(&mut self, bytes: &[u8]) -> Vec<SseEvent> {
        self.buffer.extend_from_slice(bytes);
        let mut events = Vec::new();
        let mut start = 0;
        let mut index = 0;
        while index < self.buffer.len() {
            let end = match self.buffer[index] {
                b'\n' => index + 1,
                b'\r' => match self.buffer.get(index + 1) {
                    Some(b'\n') => index + 2,
                    Some(_) => index + 1,
                    // A `\r` at the end of the chunk may be the first half of `\r\n`.
                    None => break,
                },
                _ => {
                    index += 1;
                    continue;
                }
            };
            let line = String::from_utf8_lossy(&self.buffer[start..index]).into_owned();
            events.extend(self.line(&line));
            start = end;
            index = end;
        }
        self.buffer.drain(..start);
        events
    }

    /// The body ended: flush a last unterminated line and event.
    fn finish(&mut self) -> Vec<SseEvent> {
        let mut events = Vec::new();
        if !self.buffer.is_empty() {
            let rest = std::mem::take(&mut self.buffer);
            let line = String::from_utf8_lossy(&rest);
            events.extend(self.line(line.trim_end_matches('\r')));
        }
        events.extend(self.line(""));
        events
    }

    fn line(&mut self, line: &str) -> Option<SseEvent> {
        if line.is_empty() {
            if self.event.is_none() && self.data.is_empty() {
                return None;
            }
            return Some(SseEvent {
                event: self.event.take(),
                data: std::mem::take(&mut self.data).join("\n"),
            });
        }
        if line.starts_with(':') {
            return None;
        }
        let (field, value) = line.split_once(':').unwrap_or((line, ""));
        let value = value.strip_prefix(' ').unwrap_or(value);
        match field {
            "event" => self.event = Some(value.to_owned()),
            "data" => self.data.push(value.to_owned()),
            _ => {}
        }
        None
    }
}

/// Content deltas of a streamed chat completion.
#[derive(Debug, Default)]
pub(super) struct ChatStreamParser {
    decoder: SseDecoder,
    /// `[DONE]` arrived; everything after it is ignored.
    done: bool,
}

impl ChatStreamParser {
    pub(super) fn feed(&mut self, bytes: &[u8]) -> Vec<Result<String, LlmError>> {
        let events = self.decoder.feed(bytes);
        self.deltas(events)
    }

    pub(super) fn finish(&mut self) -> Vec<Result<String, LlmError>> {
        let events = self.decoder.finish();
        self.deltas(events)
    }

    fn deltas(&mut self, events: Vec<SseEvent>) -> Vec<Result<String, LlmError>> {
        let mut out = Vec::new();
        for event in events {
            if self.done {
                continue;
            }
            if event.data.starts_with("[DONE]") {
                self.done = true;
                continue;
            }
            // Assistant-thread events never occur in chat completions.
            if event
                .event
                .as_deref()
                .is_some_and(|name| name.starts_with("thread."))
            {
                continue;
            }
            let chunk: Value = match serde_json::from_str(&event.data) {
                Ok(chunk) => chunk,
                Err(error) => {
                    out.push(Err(LlmError::InvalidResponse(format!(
                        "could not parse stream chunk {:?}: {error}",
                        event.data
                    ))));
                    return out;
                }
            };
            if let Some(error) = chunk.get("error").filter(|error| truthy(error)) {
                out.push(Err(LlmError::Api {
                    status: None,
                    error: Some(error.clone()),
                    message: None,
                }));
                return out;
            }
            let delta = chunk
                .get("choices")
                .and_then(|choices| choices.get(0))
                .and_then(|choice| choice.get("delta"))
                .and_then(|delta| delta.get("content"))
                .and_then(Value::as_str)
                .filter(|content| !content.is_empty());
            if let Some(delta) = delta {
                out.push(Ok(delta.to_owned()));
            }
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn deltas(chunks: &[&str]) -> Vec<Result<String, LlmError>> {
        let mut parser = ChatStreamParser::default();
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend(parser.feed(chunk.as_bytes()));
        }
        out.extend(parser.finish());
        out
    }

    fn content(text: &str) -> String {
        format!(
            "data: {{\"choices\":[{{\"delta\":{{\"content\":{}}}}}]}}\n\n",
            serde_json::to_string(text).unwrap()
        )
    }

    #[test]
    fn yields_content_in_order_and_stops_at_done() {
        let body = format!(
            "{}{}data: [DONE]\n\n{}",
            content("Hello"),
            content(" world"),
            content("late")
        );
        assert_eq!(
            deltas(&[&body]),
            vec![Ok("Hello".to_owned()), Ok(" world".to_owned())]
        );
    }

    #[test]
    fn handles_events_and_line_endings_split_across_chunks() {
        let body = content("a").replace('\n', "\r\n") + &content("b").replace('\n', "\r");
        let bytes = body.as_bytes();
        let chunks: Vec<String> = bytes
            .chunks(3)
            .map(|c| String::from_utf8_lossy(c).into_owned())
            .collect();
        let chunks: Vec<&str> = chunks.iter().map(String::as_str).collect();
        assert_eq!(
            deltas(&chunks),
            vec![Ok("a".to_owned()), Ok("b".to_owned())]
        );
    }

    #[test]
    fn skips_comments_empty_deltas_and_role_chunks() {
        let body = format!(
            ": keep-alive\n\ndata: {{\"choices\":[{{\"delta\":{{\"role\":\"assistant\"}}}}]}}\n\n{}{}",
            content(""),
            content("x")
        );
        assert_eq!(deltas(&[&body]), vec![Ok("x".to_owned())]);
    }

    #[test]
    fn joins_multi_line_data_and_flushes_an_unterminated_last_event() {
        let body = "data: {\"choices\":\ndata: [{\"delta\":{\"content\":\"y\"}}]}";
        assert_eq!(deltas(&[body]), vec![Ok("y".to_owned())]);
    }

    #[test]
    fn surfaces_an_error_chunk_and_stops() {
        let body = format!(
            "data: {{\"error\":{{\"message\":\"overloaded\"}}}}\n\n{}",
            content("after")
        );
        let out = deltas(&[&body]);
        assert_eq!(out.len(), 1);
        assert_eq!(
            out[0].as_ref().unwrap_err().to_string(),
            "overloaded\n{\"message\":\"overloaded\"}"
        );
    }

    #[test]
    fn surfaces_a_malformed_chunk() {
        let out = deltas(&["data: not json\n\n"]);
        assert!(matches!(
            out.as_slice(),
            [Err(LlmError::InvalidResponse(_))]
        ));
    }
}
