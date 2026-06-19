//! Offline integration tests against a local `wiremock` server. These never hit
//! the network — `wiremock` binds an ephemeral localhost port and we point the
//! client's `base_url` at it.

use std::sync::{Arc, Mutex};
use std::time::Duration;

use futures::StreamExt;
use simse::types::{ContentBlock, ContentDelta, InputMessage, MessageStreamEvent, StopReason};
use simse::{Client, MessageCreateParams};
use wiremock::matchers::{body_json_string, header, method, path};
use wiremock::{Mock, MockServer, Request, Respond, ResponseTemplate};

/// Build a client pointed at the mock server with no retries unless a test
/// overrides it.
fn client_for(server: &MockServer) -> Client {
    Client::builder()
        .api_key("sk_test_key")
        .base_url(server.uri())
        .max_retries(0)
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client builds")
}

// ════════════════════════════════════════════════════════════════════════════
// Auth headers
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn sends_both_auth_headers_and_version() {
    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "sk_test_key"))
        .and(header("authorization", "Bearer sk_test_key"))
        .and(header("anthropic-version", "2026-06-01"))
        .and(header("content-type", "application/json"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_1",
            "type": "message",
            "role": "assistant",
            "model": "zoysia",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let msg = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 16)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(msg.text(), "ok");
    // `.expect(1)` on the mock asserts exactly one matching request arrived with
    // all required headers; verified on drop.
}

/// A dynamic [`AuthProvider`]-supplied token must land on BOTH the `x-api-key`
/// header and the `Authorization: Bearer` value, overriding any static key.
#[tokio::test]
async fn auth_provider_token_lands_on_both_headers() {
    struct DynProvider;

    #[async_trait::async_trait]
    impl simse::AuthProvider for DynProvider {
        async fn token(&self) -> simse::Result<String> {
            Ok("st_dynamic_token".to_string())
        }
    }

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "st_dynamic_token"))
        .and(header("authorization", "Bearer st_dynamic_token"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_dyn",
            "type": "message",
            "role": "assistant",
            "model": "zoysia",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        })))
        .expect(1)
        .mount(&server)
        .await;

    // A static key is supplied too, to prove the provider token takes precedence.
    let client = Client::builder()
        .api_key("sk_static_should_be_ignored")
        .auth_provider(Arc::new(DynProvider))
        .base_url(server.uri())
        .max_retries(0)
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client builds");

    let msg = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 16)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(msg.text(), "ok");
}

/// A client built with only an [`AuthProvider`] (no static key, no env key)
/// still builds and authenticates.
#[tokio::test]
async fn auth_provider_without_static_key_builds_and_authenticates() {
    struct DynProvider;

    #[async_trait::async_trait]
    impl simse::AuthProvider for DynProvider {
        async fn token(&self) -> simse::Result<String> {
            Ok("st_only".to_string())
        }
    }

    let server = MockServer::start().await;

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(header("x-api-key", "st_only"))
        .and(header("authorization", "Bearer st_only"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_dyn2",
            "type": "message",
            "role": "assistant",
            "model": "zoysia",
            "content": [{"type": "text", "text": "ok"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 1, "output_tokens": 1}
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = Client::builder()
        .auth_provider(Arc::new(DynProvider))
        .base_url(server.uri())
        .max_retries(0)
        .timeout(Duration::from_secs(5))
        .build()
        .expect("client builds without a static key when a provider is set");

    let msg = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 16)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(msg.text(), "ok");
}

// ════════════════════════════════════════════════════════════════════════════
// Message create round-trip
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn message_create_round_trip() {
    let server = MockServer::start().await;

    let expected_body = serde_json::to_string(&serde_json::json!({
        "model": "zoysia",
        "messages": [{"role": "user", "content": "What is 2+2?"}],
        "max_tokens": 64,
        "system": "Be terse.",
        "stream": false
    }))
    .unwrap();

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .and(body_json_string(expected_body))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_round",
            "type": "message",
            "role": "assistant",
            "model": "zoysia",
            "content": [{"type": "text", "text": "4"}],
            "stop_reason": "end_turn",
            "stop_sequence": null,
            "usage": {"input_tokens": 8, "output_tokens": 1}
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let msg = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 64)
                .system("Be terse.")
                .message(InputMessage::user("What is 2+2?"))
                .build(),
        )
        .await
        .expect("request succeeds");

    assert_eq!(msg.id, "msg_round");
    assert_eq!(msg.text(), "4");
    assert_eq!(msg.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(msg.usage.input_tokens, 8);
    assert_eq!(msg.usage.output_tokens, 1);
}

#[tokio::test]
async fn message_create_parses_tool_use_block() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "id": "msg_t",
            "type": "message",
            "role": "assistant",
            "model": "zoysia",
            "content": [
                {"type": "text", "text": "Let me check."},
                {"type": "tool_use", "id": "toolu_42", "name": "get_weather", "input": {"city": "SF"}}
            ],
            "stop_reason": "tool_use",
            "stop_sequence": null,
            "usage": {"input_tokens": 12, "output_tokens": 9}
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let msg = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 64)
                .message(InputMessage::user("weather?"))
                .build(),
        )
        .await
        .unwrap();

    assert_eq!(msg.stop_reason, Some(StopReason::ToolUse));
    let tools = msg.tool_uses();
    assert_eq!(tools.len(), 1);
    match tools[0] {
        ContentBlock::ToolUse { name, input, id } => {
            assert_eq!(name, "get_weather");
            assert_eq!(id, "toolu_42");
            assert_eq!(input["city"], "SF");
        }
        _ => panic!("expected tool_use"),
    }
}

// ════════════════════════════════════════════════════════════════════════════
// SSE streaming + accumulation
// ════════════════════════════════════════════════════════════════════════════

const SSE_STREAM: &str = "event: message_start\n\
data: {\"type\":\"message_start\",\"message\":{\"id\":\"msg_s\",\"type\":\"message\",\"role\":\"assistant\",\"model\":\"zoysia\",\"content\":[],\"stop_reason\":null,\"stop_sequence\":null,\"usage\":{\"input_tokens\":10,\"output_tokens\":0}}}\n\n\
event: content_block_start\n\
data: {\"type\":\"content_block_start\",\"index\":0,\"content_block\":{\"type\":\"text\",\"text\":\"\"}}\n\n\
event: ping\n\
data: {\"type\":\"ping\"}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\"Hello\"}}\n\n\
event: content_block_delta\n\
data: {\"type\":\"content_block_delta\",\"index\":0,\"delta\":{\"type\":\"text_delta\",\"text\":\", world\"}}\n\n\
event: content_block_stop\n\
data: {\"type\":\"content_block_stop\",\"index\":0}\n\n\
event: message_delta\n\
data: {\"type\":\"message_delta\",\"delta\":{\"stop_reason\":\"end_turn\",\"stop_sequence\":null},\"usage\":{\"output_tokens\":42}}\n\n\
event: message_stop\n\
data: {\"type\":\"message_stop\"}\n\n";

#[tokio::test]
async fn streaming_yields_typed_events() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SSE_STREAM),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let mut stream = client
        .messages()
        .stream(
            MessageCreateParams::builder("zoysia", 64)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .unwrap();

    let mut text = String::new();
    let mut saw_start = false;
    let mut saw_stop = false;
    while let Some(ev) = stream.next().await {
        match ev.unwrap() {
            MessageStreamEvent::MessageStart { .. } => saw_start = true,
            MessageStreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text: t },
                ..
            } => text.push_str(&t),
            MessageStreamEvent::MessageStop => saw_stop = true,
            _ => {}
        }
    }
    assert!(saw_start);
    assert!(saw_stop);
    assert_eq!(text, "Hello, world");
}

#[tokio::test]
async fn streaming_accumulates_final_message() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SSE_STREAM),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let message = client
        .messages()
        .stream(
            MessageCreateParams::builder("zoysia", 64)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .unwrap()
        .accumulate()
        .await
        .unwrap();

    assert_eq!(message.id, "msg_s");
    assert_eq!(message.text(), "Hello, world");
    assert_eq!(message.stop_reason, Some(StopReason::EndTurn));
    assert_eq!(message.usage.input_tokens, 10);
    assert_eq!(message.usage.output_tokens, 42);
}

// ════════════════════════════════════════════════════════════════════════════
// Error mapping (401 / 429 / 400) — both envelopes
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn maps_401_anthropic_envelope() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(
            ResponseTemplate::new(401)
                .insert_header("request-id", "req_unauth")
                .set_body_json(serde_json::json!({
                    "type": "error",
                    "error": {"type": "authentication_error", "message": "invalid API key"},
                    "request_id": "req_unauth"
                })),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let err = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 16)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .unwrap_err();

    assert!(err.is_authentication());
    assert_eq!(err.status(), Some(401));
    assert_eq!(err.error_type(), Some("authentication_error"));
    assert_eq!(err.request_id(), Some("req_unauth"));
}

#[tokio::test]
async fn maps_400_bad_request() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "max_tokens is required"}
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let err = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 16)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .unwrap_err();

    assert!(err.is_bad_request());
    assert_eq!(err.status(), Some(400));
    assert!(!err.is_retryable());
}

#[tokio::test]
async fn maps_legacy_envelope_on_platform_surface() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/account"))
        .respond_with(ResponseTemplate::new(403).set_body_json(serde_json::json!({
            "error": {"code": "permission_denied", "message": "key lacks permission"}
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let err = client.account().retrieve().await.unwrap_err();
    assert!(err.is_permission_denied());
    assert_eq!(err.status(), Some(403));
    // The legacy `code` is surfaced through `error_type`.
    assert_eq!(err.error_type(), Some("permission_denied"));
}

// ════════════════════════════════════════════════════════════════════════════
// Retry / retry-after handling
// ════════════════════════════════════════════════════════════════════════════

/// A responder that returns 429 (with retry-after) for the first N calls, then
/// 200. Lets us assert the client retried and honored the header without real
/// network latency.
struct FlakyResponder {
    fail_count: usize,
    calls: Arc<Mutex<usize>>,
}

impl Respond for FlakyResponder {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        let mut n = self.calls.lock().unwrap();
        *n += 1;
        if *n <= self.fail_count {
            ResponseTemplate::new(429)
                .insert_header("retry-after", "0")
                .insert_header("request-id", "req_429")
                .set_body_json(serde_json::json!({
                    "type": "error",
                    "error": {"type": "rate_limit_error", "message": "slow down"}
                }))
        } else {
            ResponseTemplate::new(200).set_body_json(serde_json::json!({
                "id": "msg_ok",
                "type": "message",
                "role": "assistant",
                "model": "zoysia",
                "content": [{"type": "text", "text": "recovered"}],
                "stop_reason": "end_turn",
                "stop_sequence": null,
                "usage": {"input_tokens": 1, "output_tokens": 1}
            }))
        }
    }
}

#[tokio::test]
async fn retries_429_then_succeeds() {
    let server = MockServer::start().await;
    let calls = Arc::new(Mutex::new(0usize));

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(FlakyResponder {
            fail_count: 2,
            calls: calls.clone(),
        })
        .mount(&server)
        .await;

    // max_retries = 2 → 3 total attempts; the 3rd succeeds.
    let client = Client::builder()
        .api_key("sk_x")
        .base_url(server.uri())
        .max_retries(2)
        .build()
        .unwrap();

    let msg = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 16)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .expect("recovers after retries");

    assert_eq!(msg.text(), "recovered");
    assert_eq!(*calls.lock().unwrap(), 3, "should be 1 initial + 2 retries");
}

#[tokio::test]
async fn gives_up_after_max_retries() {
    let server = MockServer::start().await;
    let calls = Arc::new(Mutex::new(0usize));

    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(FlakyResponder {
            fail_count: 99, // always fails
            calls: calls.clone(),
        })
        .mount(&server)
        .await;

    let client = Client::builder()
        .api_key("sk_x")
        .base_url(server.uri())
        .max_retries(1)
        .build()
        .unwrap();

    let err = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 16)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .unwrap_err();

    assert!(err.is_rate_limit());
    // 1 initial + 1 retry = 2 calls.
    assert_eq!(*calls.lock().unwrap(), 2);
}

#[tokio::test]
async fn does_not_retry_non_retryable_4xx() {
    // A 400-only server: the client must NOT retry it even with retries enabled.
    let server2 = MockServer::start().await;
    let calls2 = Arc::new(Mutex::new(0usize));
    Mock::given(method("POST"))
        .and(path("/v1/messages"))
        .respond_with(Count400 {
            calls: calls2.clone(),
        })
        .mount(&server2)
        .await;

    let client = Client::builder()
        .api_key("sk_x")
        .base_url(server2.uri())
        .max_retries(3)
        .build()
        .unwrap();

    let err = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 16)
                .message(InputMessage::user("hi"))
                .build(),
        )
        .await
        .unwrap_err();
    assert!(err.is_bad_request());
    assert_eq!(*calls2.lock().unwrap(), 1, "400 must not be retried");
}

struct Count400 {
    calls: Arc<Mutex<usize>>,
}
impl Respond for Count400 {
    fn respond(&self, _req: &Request) -> ResponseTemplate {
        *self.calls.lock().unwrap() += 1;
        ResponseTemplate::new(400).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "invalid_request_error", "message": "bad"}
        }))
    }
}

// ════════════════════════════════════════════════════════════════════════════
// Models list parsing
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn models_list_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "data": [
                {"id":"zoysia","type":"model","display_name":"Zoysia (Qwen3.5 9B)","created_at":"2026-01-01T00:00:00Z","max_input_tokens":131072,"max_tokens":8192},
                {"id":"rye","type":"model","display_name":"Rye (Qwen3.5 4B)","created_at":"2026-01-01T00:00:00Z","max_input_tokens":131072,"max_tokens":8192}
            ],
            "has_more": false,
            "first_id": "zoysia",
            "last_id": "rye",
            "models": [],
            "acp_providers": []
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let list = client.models().list().await.unwrap();

    assert_eq!(list.data.len(), 2);
    assert!(!list.has_more);
    assert_eq!(list.first_id.as_deref(), Some("zoysia"));
    assert_eq!(list.last_id.as_deref(), Some("rye"));
    assert_eq!(list.data[0].id, "zoysia");
    assert_eq!(list.data[0].display_name, "Zoysia (Qwen3.5 9B)");
    assert_eq!(list.data[0].max_tokens, Some(8192));
    assert_eq!(list.data[1].id, "rye");
}

#[tokio::test]
async fn model_retrieve_not_found() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/models/ghost"))
        .respond_with(ResponseTemplate::new(404).set_body_json(serde_json::json!({
            "type": "error",
            "error": {"type": "not_found_error", "message": "model 'ghost' not found"},
            "request_id": "req_nf"
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let err = client.models().retrieve("ghost").await.unwrap_err();
    assert!(err.is_not_found());
    assert_eq!(err.request_id(), Some("req_nf"));
}

// ════════════════════════════════════════════════════════════════════════════
// count_tokens
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn count_tokens_parses() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/messages/count_tokens"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "input_tokens": 17
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let count = client
        .messages()
        .count_tokens(
            MessageCreateParams::builder("zoysia", 1)
                .message(InputMessage::user("how many tokens is this?"))
                .build(),
        )
        .await
        .unwrap();
    assert_eq!(count.input_tokens, 17);
}

// ════════════════════════════════════════════════════════════════════════════
// Session prompt (buffered + streaming, anonymous SSE shape)
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn session_prompt_buffered() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/sessions/sess_1/messages"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "message": {"role": "assistant", "content": "the answer"},
            "usage": {"input_tokens": 5, "output_tokens": 3}
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let result = client.sessions().prompt("sess_1", "question").await.unwrap();
    assert_eq!(result.message.content, "the answer");
    assert_eq!(result.usage.input_tokens, 5);
    assert_eq!(result.usage.output_tokens, 3);
    assert_eq!(result.usage.total_tokens(), 8);
}

const SESSION_SSE: &str = "data: {\"type\":\"delta\",\"delta\":\"Hel\"}\n\n\
data: {\"type\":\"delta\",\"delta\":\"lo\"}\n\n\
data: {\"type\":\"tool_call\",\"id\":\"tc_1\",\"name\":\"search\",\"input\":{\"q\":\"x\"}}\n\n\
data: {\"type\":\"tool_result\",\"id\":\"tc_1\",\"output\":\"result\",\"is_error\":false}\n\n\
data: {\"type\":\"done\",\"status\":\"complete\",\"text\":\"Hello\",\"usage\":{\"input_tokens\":1,\"output_tokens\":2}}\n\n\
data: [DONE]\n\n";

#[tokio::test]
async fn session_prompt_streaming() {
    let server = MockServer::start().await;
    Mock::given(method("POST"))
        .and(path("/v1/sessions/sess_2/messages"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("content-type", "text/event-stream")
                .set_body_string(SESSION_SSE),
        )
        .mount(&server)
        .await;

    let client = client_for(&server);
    let text = client
        .sessions()
        .stream("sess_2", "go")
        .await
        .unwrap()
        .collect_text()
        .await
        .unwrap();
    // `Done.text` is authoritative.
    assert_eq!(text, "Hello");
}

// ════════════════════════════════════════════════════════════════════════════
// Response hook surfaces request-id + rate-limit headers
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn response_hook_surfaces_meta() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/account"))
        .respond_with(
            ResponseTemplate::new(200)
                .insert_header("request-id", "req_meta")
                .insert_header("anthropic-ratelimit-requests-limit", "1000")
                .insert_header("anthropic-ratelimit-requests-remaining", "999")
                .insert_header("anthropic-ratelimit-requests-reset", "2026-06-19T00:00:00Z")
                .set_body_json(serde_json::json!({
                    "id": "pa_1", "user_id": "user_1", "plan": "free"
                })),
        )
        .mount(&server)
        .await;

    let captured = Arc::new(Mutex::new(None));
    let cap = captured.clone();
    let client = Client::builder()
        .api_key("sk_x")
        .base_url(server.uri())
        .on_response(Arc::new(move |meta| {
            *cap.lock().unwrap() = Some((
                meta.request_id.clone(),
                meta.requests_limit,
                meta.requests_remaining,
                meta.requests_reset.clone(),
            ));
        }))
        .build()
        .unwrap();

    let account = client.account().retrieve().await.unwrap();
    assert_eq!(account.plan, "free");

    let meta = captured.lock().unwrap().clone().expect("hook fired");
    assert_eq!(meta.0.as_deref(), Some("req_meta"));
    assert_eq!(meta.1, Some(1000));
    assert_eq!(meta.2, Some(999));
    assert_eq!(meta.3.as_deref(), Some("2026-06-19T00:00:00Z"));
}

// ════════════════════════════════════════════════════════════════════════════
// Agents — subagent run history (GET /v1/agents)
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn agents_list_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/agents"))
        .and(header("x-api-key", "sk_test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "agents": [
                {
                    "id": "agent-1",
                    "description": "build the SDK",
                    "status": "completed",
                    "started_at": "2026-06-19T00:00:00Z",
                    "completed_at": "2026-06-19T00:03:20Z",
                    "duration_ms": 200000,
                    "turns": 6,
                    "input_tokens": 1500,
                    "output_tokens": 900,
                    "error": ""
                },
                {
                    "id": "agent-2",
                    "description": "still going",
                    "status": "running",
                    "started_at": "2026-06-19T00:10:00Z",
                    "completed_at": "",
                    "duration_ms": 0,
                    "turns": 2,
                    "input_tokens": 400,
                    "output_tokens": 0,
                    "error": ""
                }
            ]
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let list = client.agents().list().await.expect("agents list succeeds");

    assert_eq!(list.agents.len(), 2);
    let a = &list.agents[0];
    assert_eq!(a.id, "agent-1");
    assert_eq!(a.description, "build the SDK");
    assert_eq!(a.status, "completed");
    assert_eq!(a.completed_at, "2026-06-19T00:03:20Z");
    assert_eq!(a.duration_ms, 200_000);
    assert_eq!(a.turns, 6);
    assert_eq!(a.input_tokens, 1500);
    assert_eq!(a.output_tokens, 900);

    let b = &list.agents[1];
    assert_eq!(b.status, "running");
    assert_eq!(b.completed_at, "");
    assert_eq!(b.duration_ms, 0);
    assert_eq!(b.output_tokens, 0);
}

// ════════════════════════════════════════════════════════════════════════════
// Usage dashboard — the rich UsagePanel view (GET /v1/usage/dashboard)
// ════════════════════════════════════════════════════════════════════════════

#[tokio::test]
async fn usage_dashboard_parses_camelcase_wire() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/usage/dashboard"))
        .and(header("x-api-key", "sk_test_key"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "plan": "pro",
            "periodStart": 1_750_000_000_000_i64,
            "models": [{
                "model": "zoysia",
                "includedInputTokens": 12000,
                "includedOutputTokens": 8000,
                "extraInputTokens": 100,
                "extraOutputTokens": 50,
                "extraSpendCents": 9,
                "requestCount": 42,
                "multiplier": 2.0
            }],
            "billing": {
                "extraUsageEnabled": true,
                "extraUsageCapCents": 10000,
                "creditsBalanceCents": 500,
                "extraSpendThisPeriodCents": 9,
                "planIncludedTokens": 1_000_000_i64
            },
            "compute": {
                "session": {
                    "current": {
                        "state": "active",
                        "startedAt": 1_750_000_000_000_i64,
                        "expiresAt": 1_750_018_000_000_i64,
                        "cooldownUntil": 0,
                        "usedMs": 3_600_000,
                        "limitMs": 18_000_000,
                        "inFlight": 0
                    }
                }
            }
        })))
        .expect(1)
        .mount(&server)
        .await;

    let client = client_for(&server);
    let d = client
        .usage()
        .dashboard()
        .await
        .expect("usage dashboard succeeds");

    assert_eq!(d.plan, "pro");
    assert_eq!(d.period_start, 1_750_000_000_000);
    assert_eq!(d.models.len(), 1);
    assert_eq!(d.models[0].model, "zoysia");
    assert_eq!(d.models[0].included_input_tokens, 12_000);
    assert_eq!(d.models[0].extra_spend_cents, 9);
    assert_eq!(d.models[0].request_count, 42);
    assert_eq!(d.models[0].multiplier, 2.0);

    assert!(d.billing.extra_usage_enabled);
    assert_eq!(d.billing.extra_usage_cap_cents, 10_000);
    assert_eq!(d.billing.credits_balance_cents, 500);
    assert_eq!(d.billing.plan_included_tokens, 1_000_000);

    let win = d
        .compute
        .expect("compute block present")
        .session
        .expect("session present")
        .current
        .expect("current window present");
    assert_eq!(win.state, "active");
    assert_eq!(win.used_ms, 3_600_000);
    assert_eq!(win.limit_ms, 18_000_000);
    assert_eq!(win.in_flight, 0);
}

/// When tally is unbound warp omits the compute block (`compute: null`) and
/// degrades the payments-sourced blocks to their zero defaults; the panel still
/// deserializes.
#[tokio::test]
async fn usage_dashboard_omitted_compute_still_parses() {
    let server = MockServer::start().await;
    Mock::given(method("GET"))
        .and(path("/v1/usage/dashboard"))
        .respond_with(ResponseTemplate::new(200).set_body_json(serde_json::json!({
            "plan": "free",
            "periodStart": 1_750_000_000_000_i64,
            "models": [],
            "billing": {
                "extraUsageEnabled": false,
                "extraUsageCapCents": 0,
                "creditsBalanceCents": 0,
                "extraSpendThisPeriodCents": 0,
                "planIncludedTokens": 200000
            },
            "compute": null
        })))
        .mount(&server)
        .await;

    let client = client_for(&server);
    let d = client.usage().dashboard().await.unwrap();
    assert_eq!(d.plan, "free");
    assert!(d.models.is_empty());
    assert!(!d.billing.extra_usage_enabled);
    assert_eq!(d.billing.plan_included_tokens, 200_000);
    assert!(d.compute.is_none());
}
