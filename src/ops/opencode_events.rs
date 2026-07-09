use std::{
    io::{BufRead, BufReader},
    sync::mpsc::Sender,
    thread,
    time::Duration,
};

/// A `session.created` event parsed from the opencode serve SSE stream.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionCreatedEvent {
    pub session_id: String,
    pub parent_id: Option<String>,
}

/// Parse a single SSE `data:` payload into a `SessionCreatedEvent`.
///
/// Returns `None` if the payload is not a `session.created` event or is
/// malformed.  The payload should be the JSON text *after* the `data: ` prefix.
fn parse_sse_data_payload(payload: &str) -> Option<SessionCreatedEvent> {
    let json: serde_json::Value = serde_json::from_str(payload.trim()).ok()?;

    let event_type = json.get("type")?.as_str()?;
    if event_type != "session.created" {
        return None;
    }

    let info = json.pointer("/properties/info")?;
    let session_id = info.get("id")?.as_str()?.to_string();
    let parent_id = info
        .get("parentID")
        .and_then(|v| v.as_str())
        .map(|s| s.to_string());

    Some(SessionCreatedEvent {
        session_id,
        parent_id,
    })
}

/// Parse a raw SSE line into a `SessionCreatedEvent`.
///
/// Accepts lines in these forms:
/// - `data: {...json...}` — a data frame
/// - `:` + anything — SSE comment / keepalive (ignored)
/// - empty line — event delimiter (ignored)
/// - any other line — ignored (e.g. `event:`, `id:`, `retry:`)
pub fn parse_sse_data_line(line: &str) -> Option<SessionCreatedEvent> {
    let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');

    if trimmed.is_empty() || trimmed.starts_with(':') {
        return None;
    }

    let payload = trimmed.strip_prefix("data:")?;
    let payload = payload.strip_prefix(' ').unwrap_or(payload);

    if payload.is_empty() {
        return None;
    }

    parse_sse_data_payload(payload)
}

/// Consume a BufRead SSE stream, invoking `handler` for each `session.created`
/// event encountered.
///
/// Each SSE event is delimited by a blank line.  Data frames for a single
/// event are concatenated with `\n` before parsing, per the SSE spec.
pub fn consume_sse_stream<R: BufRead, F: FnMut(SessionCreatedEvent)>(
    mut reader: R,
    mut handler: F,
) -> std::io::Result<()> {
    let mut data_lines: Vec<String> = Vec::new();

    loop {
        let mut line = String::new();
        if reader.read_line(&mut line)? == 0 {
            break;
        }

        let trimmed = line.trim_end_matches('\n').trim_end_matches('\r');

        if trimmed.is_empty() {
            if !data_lines.is_empty() {
                let payload = data_lines.join("\n");
                if let Some(event) = parse_sse_data_payload(&payload) {
                    handler(event);
                }
                data_lines.clear();
            }
            continue;
        }

        if let Some(data) = trimmed.strip_prefix("data:") {
            let data = data.strip_prefix(' ').unwrap_or(data);
            data_lines.push(data.to_string());
        }
    }

    if !data_lines.is_empty() {
        let payload = data_lines.join("\n");
        if let Some(event) = parse_sse_data_payload(&payload) {
            handler(event);
        }
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// SSE subscriber thread
// ---------------------------------------------------------------------------

/// Handle to a background SSE subscriber thread.
pub struct SessionEventSubscriber {
    stop_tx: Sender<()>,
    // Held so the JoinHandle is dropped (detaching the thread) when the
    // SessionEventSubscriber is dropped. The thread cannot be joined at
    // shutdown — it is blocked on a streaming SSE read and serves persist
    // past ocmux exit — so we intentionally never read this field.
    #[allow(dead_code)]
    join_handle: Option<thread::JoinHandle<()>>,
}

impl SessionEventSubscriber {
    /// Start a background thread that connects to `GET /event` on the given
    /// serve port, parses SSE frames, and sends `SessionCreatedEvent`s over
    /// the provided channel tagged with the serve port.
    ///
    /// The thread reconnects with capped backoff on disconnect.
    pub fn start(port: u16, tx: Sender<(u16, SessionCreatedEvent)>) -> Self {
        let (stop_tx, stop_rx) = std::sync::mpsc::channel();
        let url = format!("http://localhost:{port}/event");

        let join_handle = thread::spawn(move || {
            let mut backoff = Duration::from_millis(500);

            loop {
                if stop_rx.try_recv().is_ok() {
                    break;
                }

                let connected = || -> Result<(), ()> {
                    let client = reqwest::blocking::Client::builder()
                        .timeout(Duration::from_secs(60))
                        .build()
                        .map_err(|_| ())?;

                    let resp = client.get(&url).send().map_err(|_| ())?;
                    if !resp.status().is_success() {
                        return Err(());
                    }

                    let reader = BufReader::new(resp);
                    let tx_clone = tx.clone();
                    let port_clone = port;
                    consume_sse_stream(reader, |event| {
                        let _ = tx_clone.send((port_clone, event));
                    })
                    .map_err(|_| ())?;

                    Ok(())
                };

                if connected().is_ok() {
                    backoff = Duration::from_millis(500);
                }

                if stop_rx.recv_timeout(backoff).is_ok() {
                    break;
                }
                backoff = (backoff * 2).min(Duration::from_secs(5));
            }
        });

        SessionEventSubscriber {
            stop_tx,
            join_handle: Some(join_handle),
        }
    }

    // `stop()` was removed: it joined the background thread, which is
    // blocked on a streaming SSE read that only returns when the serve
    // closes the connection. Serve daemons persist past ocmux exit, so
    // joining at shutdown would hang indefinitely. Shutdown is now handled
    // by `Drop` below, which sends a best-effort stop signal and detaches
    // the thread; the OS reclaims it on process exit.
}

impl Drop for SessionEventSubscriber {
    fn drop(&mut self) {
        // Best-effort stop signal. We intentionally do NOT join: the thread
        // is blocked on a streaming SSE read (read_line) that only returns
        // when the serve closes the connection. Serve daemons persist past
        // ocmux exit, so joining would hang indefinitely. The JoinHandle
        // field drops here, detaching the thread; it dies when the process
        // exits.
        let _ = self.stop_tx.send(());
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    #[test]
    fn parse_session_created_extracts_id_and_parent_id() {
        let line = r#"data: {"type":"session.created","properties":{"info":{"id":"sess_abc","parentID":null}}}"#;
        let event = parse_sse_data_line(line).expect("should parse");
        assert_eq!(event.session_id, "sess_abc");
        assert_eq!(event.parent_id, None);
    }

    #[test]
    fn parse_session_created_with_parent_id() {
        let line = r#"data: {"type":"session.created","properties":{"info":{"id":"sess_child","parentID":"sess_parent"}}}"#;
        let event = parse_sse_data_line(line).expect("should parse");
        assert_eq!(event.session_id, "sess_child");
        assert_eq!(event.parent_id, Some("sess_parent".into()));
    }

    #[test]
    fn parse_ignores_non_session_created_events() {
        assert!(
            parse_sse_data_line(
                r#"data: {"type":"session.updated","properties":{"info":{"id":"sess_abc"}}}"#
            )
            .is_none()
        );
        assert!(
            parse_sse_data_line(
                r#"data: {"type":"session.idle","properties":{"info":{"id":"sess_abc"}}}"#
            )
            .is_none()
        );
        assert!(
            parse_sse_data_line(
                r#"data: {"type":"message.updated","properties":{"info":{"id":"sess_abc"}}}"#
            )
            .is_none()
        );
        assert!(
            parse_sse_data_line(r#"data: {"type":"server.connected","properties":{}}"#).is_none()
        );
    }

    #[test]
    fn parse_ignores_malformed_json() {
        assert!(parse_sse_data_line("data: not json").is_none());
        assert!(parse_sse_data_line("data: {}").is_none());
        assert!(
            parse_sse_data_line(r#"data: {"type":"session.created","properties":{}}"#).is_none()
        );
        assert!(
            parse_sse_data_line(r#"data: {"type":"session.created","properties":{"info":{}}}"#)
                .is_none()
        );
    }

    #[test]
    fn parse_ignores_sse_comments_and_heartbeats() {
        assert!(parse_sse_data_line(": keepalive").is_none());
        assert!(parse_sse_data_line(":this is a comment").is_none());
        assert!(parse_sse_data_line("").is_none());
        assert!(parse_sse_data_line("event: session.created").is_none());
        assert!(parse_sse_data_line("id: 12345").is_none());
        assert!(parse_sse_data_line("retry: 3000").is_none());
    }

    #[test]
    fn consume_sse_stream_emits_session_created_events() {
        let stream = "\
data: {\"type\":\"server.connected\",\"properties\":{}}

data: {\"type\":\"session.created\",\"properties\":{\"info\":{\"id\":\"sess_1\",\"parentID\":null}}}

data: {\"type\":\"session.updated\",\"properties\":{\"info\":{\"id\":\"sess_1\"}}}

data: {\"type\":\"session.created\",\"properties\":{\"info\":{\"id\":\"sess_2\",\"parentID\":\"sess_1\"}}}

";

        let mut events = Vec::new();
        consume_sse_stream(Cursor::new(stream), |e| events.push(e)).unwrap();

        assert_eq!(events.len(), 2);
        assert_eq!(events[0].session_id, "sess_1");
        assert_eq!(events[0].parent_id, None);
        assert_eq!(events[1].session_id, "sess_2");
        assert_eq!(events[1].parent_id, Some("sess_1".into()));
    }
}
