//! AI processing for clients: this server passes their chat requests on to a
//! language model it has been pointed at.
//!
//! The model does not run inside this server. `[llm] endpoint` names any
//! OpenAI-compatible service (Ollama or llama.cpp on this machine, or a hosted
//! provider), and clients that choose "use the server" for AI processing send
//! their requests here instead of each needing that service's address and
//! key. The prompts stay the client's own; the model is this server's choice.

use std::time::Duration;

use anyhow::{Context, Result};
use axum::body::Bytes;
use axum::http::StatusCode;
use serde_json::Value;
use tracing::{info, warn};

use crate::config::LlmConfig;

/// How long the model's service gets to accept a connection.
const CONNECT_TIMEOUT: Duration = Duration::from_secs(3);

/// What went wrong passing a request on, in words for the client.
#[derive(Debug)]
pub enum RelayError {
    /// The request itself cannot be passed on.
    Invalid(String),
    /// The model's service could not be reached or did not answer.
    Upstream(String),
}

/// The language model service behind this server.
pub struct LlmRelay {
    http: reqwest::Client,
    base: String,
    api_key: Option<String>,
    model: String,
}

impl LlmRelay {
    /// Set up the relay and look at the service once, so a wrong address or
    /// model name shows in the log at startup. Neither is fatal: the service
    /// may simply start after this server does.
    pub async fn prepare(config: &LlmConfig) -> Result<Self> {
        let endpoint = config.endpoint.trim();
        if endpoint.is_empty() {
            anyhow::bail!(
                "AI processing is on but llm.endpoint is empty. Set it to the address of an \
                 OpenAI-compatible service, such as http://localhost:11434 for Ollama on this machine \
                 (or VOXCTRL_SERVER_LLM_ENDPOINT)."
            );
        }
        if config.model.trim().is_empty() {
            anyhow::bail!(
                "AI processing is on but llm.model is empty. Set it to a model the service at {endpoint} \
                 offers (or VOXCTRL_SERVER_LLM_MODEL)."
            );
        }
        let http = reqwest::Client::builder()
            .connect_timeout(CONNECT_TIMEOUT)
            .timeout(Duration::from_secs(config.timeout_secs.max(1)))
            .build()
            .context("Could not set up the connection to the language model")?;
        let relay = Self {
            http,
            base: api_base(endpoint),
            api_key: Some(config.api_key.trim().to_string()).filter(|k| !k.is_empty()),
            model: config.model.trim().to_string(),
        };
        relay.look().await;
        Ok(relay)
    }

    pub fn model(&self) -> &str {
        &self.model
    }

    fn with_key(&self, request: reqwest::RequestBuilder) -> reqwest::RequestBuilder {
        match &self.api_key {
            Some(key) => request.bearer_auth(key),
            None => request,
        }
    }

    async fn look(&self) {
        let url = format!("{}/models", self.base);
        match self.with_key(self.http.get(&url)).timeout(Duration::from_secs(5)).send().await {
            Ok(response) if response.status().is_success() => {
                let listed = response.json::<Value>().await.ok().and_then(|v| model_ids(&v));
                match listed {
                    Some(ids) if !ids.iter().any(|id| id == &self.model) => warn!(
                        "The language model service at {} does not list the model '{}'. It offers: {}",
                        self.base,
                        self.model,
                        if ids.is_empty() { "nothing".to_string() } else { ids.join(", ") }
                    ),
                    _ => info!("Language model service reachable at {}", self.base),
                }
            }
            Ok(response) => warn!(
                "The language model service at {} answered HTTP {} when asked for its models. \
                 AI processing requests may fail; check llm.endpoint and llm.api_key.",
                self.base,
                response.status().as_u16()
            ),
            Err(e) => warn!(
                "The language model service at {} is not reachable yet ({}). \
                 AI processing requests will fail until it is.",
                self.base,
                if e.is_connect() { "connection refused or no route" } else { "no answer" }
            ),
        }
    }

    /// Pass one chat request on and return the service's answer as it came,
    /// status included, so a client sees the service's own error messages.
    pub async fn relay(&self, mut request: Value) -> std::result::Result<(StatusCode, Bytes), RelayError> {
        let Some(fields) = request.as_object_mut() else {
            return Err(RelayError::Invalid("The request must be a JSON object with a \"messages\" list.".into()));
        };
        if !fields.get("messages").is_some_and(|m| m.as_array().is_some_and(|a| !a.is_empty())) {
            return Err(RelayError::Invalid("The request needs a non-empty \"messages\" list.".into()));
        }
        // The model is this server's choice, whatever the client named. And
        // the answer is passed back whole, so streaming is not offered.
        fields.insert("model".into(), Value::String(self.model.clone()));
        fields.insert("stream".into(), Value::Bool(false));

        let url = format!("{}/chat/completions", self.base);
        let response = self.with_key(self.http.post(&url)).json(&request).send().await.map_err(|e| {
            RelayError::Upstream(if e.is_connect() {
                "This server could not reach its language model service. Check that it is running \
                 and that llm.endpoint on the server is right."
                    .into()
            } else if e.is_timeout() {
                "The server's language model did not answer in time. A larger llm.timeout_secs on the \
                 server gives it longer."
                    .into()
            } else {
                "The request to the server's language model failed. See the server log.".into()
            })
        })?;
        let status = StatusCode::from_u16(response.status().as_u16()).unwrap_or(StatusCode::BAD_GATEWAY);
        let body = response.bytes().await.map_err(|_| {
            RelayError::Upstream("The answer from the server's language model could not be read.".into())
        })?;
        Ok((status, body))
    }
}

/// An OpenAI-style base URL ending in `/v1`, from an address given with or
/// without it.
fn api_base(endpoint: &str) -> String {
    let trimmed = endpoint.trim().trim_end_matches('/');
    if trimmed.ends_with("/v1") {
        trimmed.to_string()
    } else {
        format!("{trimmed}/v1")
    }
}

fn model_ids(models: &Value) -> Option<Vec<String>> {
    let data = models.get("data")?.as_array()?;
    Some(data.iter().filter_map(|m| m.get("id")?.as_str().map(str::to_string)).collect())
}

/// The total length of the message texts in a chat request, for the log.
pub fn message_chars(request: &Value) -> usize {
    request
        .get("messages")
        .and_then(Value::as_array)
        .map(|messages| {
            messages.iter().filter_map(|m| m.get("content")?.as_str()).map(|text| text.chars().count()).sum()
        })
        .unwrap_or(0)
}

#[cfg(test)]
pub mod testing {
    //! A stand-in language model service for tests: answers chat requests by
    //! describing what it was asked, and records each request it saw.

    use std::sync::{Arc, Mutex};

    use axum::extract::State;
    use axum::http::{HeaderMap, StatusCode};
    use axum::routing::{get, post};
    use axum::{Json, Router};
    use serde_json::{json, Value};

    #[derive(Default)]
    pub struct Seen {
        pub requests: Mutex<Vec<Value>>,
        pub keys: Mutex<Vec<Option<String>>>,
    }

    /// Start the service; returns its address and what it has seen.
    pub async fn fake_service(models: &'static [&'static str]) -> (String, Arc<Seen>) {
        let seen = Arc::new(Seen::default());
        let app = Router::new()
            .route(
                "/v1/models",
                get(move || async move {
                    Json(json!({ "data": models.iter().map(|id| json!({ "id": id })).collect::<Vec<_>>() }))
                }),
            )
            .route("/v1/chat/completions", post(chat))
            .with_state(seen.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let address = format!("http://{}", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await });
        (address, seen)
    }

    async fn chat(
        State(seen): State<Arc<Seen>>,
        headers: HeaderMap,
        Json(body): Json<Value>,
    ) -> (StatusCode, Json<Value>) {
        let key = headers.get("authorization").and_then(|v| v.to_str().ok()).map(str::to_string);
        seen.keys.lock().unwrap().push(key);
        let model = body["model"].as_str().unwrap_or_default().to_string();
        let last = body["messages"].as_array().and_then(|m| m.last()).and_then(|m| m["content"].as_str()).unwrap_or("");
        let reply = format!("[{model}] {}", last.to_uppercase());
        seen.requests.lock().unwrap().push(body.clone());
        if model == "missing" {
            return (StatusCode::NOT_FOUND, Json(json!({ "error": { "message": "model 'missing' not found" } })));
        }
        (StatusCode::OK, Json(json!({ "choices": [{ "message": { "role": "assistant", "content": reply } }] })))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn config(endpoint: &str, model: &str) -> LlmConfig {
        LlmConfig {
            enabled: true,
            endpoint: endpoint.into(),
            model: model.into(),
            api_key: "sk-upstream".into(),
            timeout_secs: 10,
        }
    }

    #[tokio::test]
    async fn a_request_is_passed_on_with_the_servers_model_and_key() {
        let (address, seen) = testing::fake_service(&["llama3.2:1b"]).await;
        let relay = LlmRelay::prepare(&config(&address, "llama3.2:1b")).await.unwrap();
        let request = json!({ "model": "the-clients-choice", "stream": true, "messages": [{ "role": "user", "content": "hello" }] });
        let (status, body) = relay.relay(request).await.unwrap();
        assert_eq!(status, StatusCode::OK);
        let body: Value = serde_json::from_slice(&body).unwrap();
        assert_eq!(body["choices"][0]["message"]["content"], "[llama3.2:1b] HELLO");
        let sent = &seen.requests.lock().unwrap()[0];
        assert_eq!(sent["model"], "llama3.2:1b");
        assert_eq!(sent["stream"], false);
        assert_eq!(seen.keys.lock().unwrap()[0].as_deref(), Some("Bearer sk-upstream"));
    }

    #[tokio::test]
    async fn the_services_own_errors_reach_the_client_unchanged() {
        let (address, _seen) = testing::fake_service(&["llama3.2:1b"]).await;
        let relay = LlmRelay::prepare(&config(&address, "missing")).await.unwrap();
        let (status, body) = relay.relay(json!({ "messages": [{ "role": "user", "content": "hi" }] })).await.unwrap();
        assert_eq!(status, StatusCode::NOT_FOUND);
        assert!(String::from_utf8_lossy(&body).contains("model 'missing' not found"));
    }

    #[tokio::test]
    async fn a_service_that_is_down_is_explained_not_fatal() {
        let closed = {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}", listener.local_addr().unwrap())
        };
        // Starting with the service down is allowed: it may come up later.
        let relay = LlmRelay::prepare(&config(&closed, "llama3.2:1b")).await.unwrap();
        let error = relay.relay(json!({ "messages": [{ "role": "user", "content": "hi" }] })).await.unwrap_err();
        assert!(
            matches!(&error, RelayError::Upstream(m) if m.contains("could not reach its language model")),
            "{error:?}"
        );
    }

    #[tokio::test]
    async fn what_is_not_a_chat_request_is_refused_before_it_is_sent() {
        let (address, seen) = testing::fake_service(&[]).await;
        let relay = LlmRelay::prepare(&config(&address, "m")).await.unwrap();
        for bad in [json!("text"), json!({}), json!({ "messages": [] }), json!({ "messages": "hi" })] {
            assert!(matches!(relay.relay(bad).await, Err(RelayError::Invalid(_))));
        }
        assert!(seen.requests.lock().unwrap().is_empty());
    }

    #[tokio::test]
    async fn turning_it_on_without_an_address_or_model_says_what_to_set() {
        let no_address = LlmRelay::prepare(&config(" ", "m")).await.err().unwrap().to_string();
        assert!(no_address.contains("llm.endpoint is empty"), "{no_address}");
        let no_model = LlmRelay::prepare(&config("http://localhost:11434", "")).await.err().unwrap().to_string();
        assert!(no_model.contains("llm.model is empty"), "{no_model}");
    }

    #[test]
    fn addresses_are_completed_and_message_lengths_counted() {
        assert_eq!(api_base("http://localhost:11434/"), "http://localhost:11434/v1");
        assert_eq!(api_base("https://api.openai.com/v1"), "https://api.openai.com/v1");
        let request = json!({ "messages": [{ "content": "abc" }, { "content": "de" }, { "content": 5 }] });
        assert_eq!(message_chars(&request), 5);
    }
}
