use std::time::Duration;

use crate::playbook::engine::{EngineError, PromptClient, SendRequest};

pub struct ServePromptClient;

pub fn parse_session_id(body: &str) -> Result<String, EngineError> {
    serde_json::from_str::<serde_json::Value>(body)
        .ok()
        .and_then(|json| json.get("id").and_then(|id| id.as_str()).map(str::to_owned))
        .ok_or_else(|| EngineError::SendFailed("session response did not include an id".into()))
}

pub fn create_session(port: u16) -> Result<String, EngineError> {
    let client = reqwest::blocking::Client::builder()
        .timeout(Duration::from_secs(5))
        .build()
        .map_err(|error| EngineError::SendFailed(error.to_string()))?;
    let response = client
        .post(format!("http://localhost:{port}/session"))
        .json(&serde_json::json!({}))
        .send()
        .map_err(|error| EngineError::SendFailed(error.to_string()))?;
    let status = response.status();
    let body = response
        .text()
        .map_err(|error| EngineError::SendFailed(error.to_string()))?;
    if !(200..300).contains(&status.as_u16()) {
        return Err(EngineError::SendFailed(format!(
            "HTTP {}: {}",
            status.as_u16(),
            body.chars().take(500).collect::<String>()
        )));
    }
    parse_session_id(&body)
}

pub fn classify_http(status: u16, body: &str) -> Result<(), EngineError> {
    if (200..300).contains(&status) {
        Ok(())
    } else {
        Err(EngineError::SendFailed(format!(
            "HTTP {status}: {}",
            body.chars().take(500).collect::<String>()
        )))
    }
}

impl PromptClient for ServePromptClient {
    fn send(&self, request: SendRequest) -> Result<(), EngineError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(request.timeout)
            .build()
            .map_err(|error| EngineError::SendFailed(error.to_string()))?;
        let response = client
            .post(prompt_url(request.port, &request.session_id))
            .json(&prompt_body(&request.agent, &request.message))
            .send()
            .map_err(|error| {
                if error.is_timeout() {
                    EngineError::TimedOut
                } else {
                    EngineError::SendFailed(error.to_string())
                }
            })?;
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        classify_http(status, &body)
    }

    fn abort(&self, session_id: &str, port: u16) -> Result<(), EngineError> {
        let client = reqwest::blocking::Client::builder()
            .timeout(Duration::from_secs(2))
            .build()
            .map_err(|error| EngineError::SendFailed(error.to_string()))?;
        let response = client
            .post(abort_url(port, session_id))
            .send()
            .map_err(|error| EngineError::SendFailed(error.to_string()))?;
        let status = response.status().as_u16();
        let body = response.text().unwrap_or_default();
        classify_http(status, &body)
    }

    fn after_send(&self, session_id: &str) -> Result<(), EngineError> {
        let name = crate::data::db::reader::DbReader::open_default()
            .and_then(|reader| reader.latest_assistant_error_name(session_id))
            .map_err(|error| EngineError::SendFailed(error.to_string()))?;
        match name.as_deref() {
            None => Ok(()),
            Some("MessageAbortedError") => Err(EngineError::Aborted),
            Some(other) => Err(EngineError::SendFailed(other.to_string())),
        }
    }
}

pub fn prompt_url(port: u16, session_id: &str) -> String {
    format!("http://localhost:{port}/session/{session_id}/message")
}

pub fn abort_url(port: u16, session_id: &str) -> String {
    format!("http://localhost:{port}/session/{session_id}/abort")
}

pub fn prompt_body(agent: &str, text: &str) -> serde_json::Value {
    serde_json::json!({
        "agent": agent,
        "parts": [{ "type": "text", "text": text }]
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_url_uses_session_id() {
        assert_eq!(
            prompt_url(4200, "sess_1"),
            "http://localhost:4200/session/sess_1/message"
        );
    }

    #[test]
    fn parse_session_id_reads_created_session() {
        assert_eq!(parse_session_id(r#"{"id":"ses_new"}"#).unwrap(), "ses_new");
    }

    #[test]
    fn abort_url_uses_session_id() {
        assert_eq!(
            abort_url(4200, "sess_1"),
            "http://localhost:4200/session/sess_1/abort"
        );
    }

    #[test]
    fn prompt_body_includes_agent_and_text_part() {
        let body = prompt_body("chat", "hello");
        assert_eq!(body["agent"], "chat");
        assert_eq!(body["parts"][0]["type"], "text");
        assert_eq!(body["parts"][0]["text"], "hello");
    }

    #[test]
    fn classify_http_accepts_success() {
        assert!(classify_http(200, "").is_ok());
    }

    #[test]
    fn classify_http_reports_status_and_body() {
        let error = classify_http(503, "unavailable").unwrap_err();
        let message = error.to_string();
        assert!(message.contains("503"));
        assert!(message.contains("unavailable"));
    }
}
