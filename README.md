# simse — Rust SDK for the Simse API

The official Rust SDK for [`api.simse.dev`](https://api.simse.dev), the
Anthropic-Messages-compatible LLM gateway. The ergonomics mirror the official
Anthropic SDKs: `client.messages().create(...)` / `.stream(...)` /
`.count_tokens(...)`, `client.models().list()` / `.retrieve(id)`, a typed error
hierarchy, and automatic retries with backoff.

Backend models: `rye` (Qwen3.5-4B) and `zoysia` (Qwen3.5-9B, default).

## Install

```toml
[dependencies]
simse = "0.1"
tokio = { version = "1", features = ["macros", "rt-multi-thread"] }
futures = "0.3" # only if you consume streams
```

TLS is rustls by default. To use the system TLS instead:

```toml
simse = { version = "0.1", default-features = false, features = ["native-tls"] }
```

## Authentication

The client sends **both** `x-api-key: <key>` and `Authorization: Bearer <key>`,
plus `anthropic-version: 2026-06-01`. The key is read from `SIMSE_API_KEY` (then
`ANTHROPIC_API_KEY`) when not passed explicitly; the base URL defaults to
`https://api.simse.dev` (override via `SIMSE_BASE_URL` or the builder).

```rust
use simse::Client;

// From the environment:
let client = Client::from_env()?;

// Or explicitly, with overrides:
let client = Client::builder()
    .api_key("sk_...")
    .base_url("https://api.simse.dev")
    .timeout(std::time::Duration::from_secs(30))
    .max_retries(2)
    .build()?;
```

### Dynamic auth (short-lived tokens)

Instead of a static API key, register an `AuthProvider` to supply a **short-lived
token that refreshes per request** (e.g. the Simse CLI exchanging its local
session for a rotating token). The provider's `token()` is awaited before every
request and its result is sent as BOTH `x-api-key` and `Authorization: Bearer`.
With a provider set, a static `api_key` is optional.

```rust
use std::sync::Arc;
use simse::{AuthProvider, Client, Result};

struct CliTokenProvider;

#[async_trait::async_trait]
impl AuthProvider for CliTokenProvider {
    async fn token(&self) -> Result<String> {
        // Mint/refresh a short-lived token here (cache + refresh internally
        // if minting is expensive — this is called once per request).
        Ok("st_short_lived_token".to_string())
    }
}

let client = Client::builder()
    .auth_provider(Arc::new(CliTokenProvider))
    .build()?;
```

## Create a message

```rust
use simse::{Client, MessageCreateParams, types::InputMessage};

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let message = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 1024)
                .system("You are a helpful assistant.")
                .message(InputMessage::user("What is the capital of France?"))
                .temperature(0.7)
                .build(),
        )
        .await?;

    println!("{}", message.text());
    println!("usage: {:?}", message.usage);
    Ok(())
}
```

## Streaming

`messages().stream(...)` returns a `Stream` of typed `MessageStreamEvent`s. The
same handle accumulates a final `Message` — call `.accumulate().await` instead of
iterating if you only want the result.

```rust
use futures::StreamExt;
use simse::{Client, MessageCreateParams};
use simse::types::{ContentDelta, InputMessage, MessageStreamEvent};

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let mut stream = client
        .messages()
        .stream(
            MessageCreateParams::builder("zoysia", 1024)
                .message(InputMessage::user("Write a haiku about Rust."))
                .build(),
        )
        .await?;

    while let Some(event) = stream.next().await {
        match event? {
            MessageStreamEvent::ContentBlockDelta {
                delta: ContentDelta::TextDelta { text },
                ..
            } => print!("{text}"),
            MessageStreamEvent::MessageStop => println!(),
            _ => {}
        }
    }
    Ok(())
}
```

Accumulate straight to a final `Message`:

```rust
let message = client.messages().stream(params).await?.accumulate().await?;
println!("{}", message.text());
```

## Tool use

```rust
use serde_json::json;
use simse::{Client, MessageCreateParams};
use simse::types::{ContentBlock, InputMessage, Role, Tool};

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let weather = Tool::new(
        "get_weather",
        json!({
            "type": "object",
            "properties": { "city": { "type": "string" } },
            "required": ["city"]
        }),
    )
    .with_description("Get the current weather for a city.");

    let message = client
        .messages()
        .create(
            MessageCreateParams::builder("zoysia", 1024)
                .tool(weather)
                .message(InputMessage::user("What's the weather in San Francisco?"))
                .build(),
        )
        .await?;

    for block in &message.content {
        if let ContentBlock::ToolUse { name, input, id } = block {
            println!("call {name}({input}) [{id}]");

            // Run the tool, then send the result back:
            let follow_up = client
                .messages()
                .create(
                    MessageCreateParams::builder("zoysia", 1024)
                        .message(InputMessage::user("What's the weather in San Francisco?"))
                        .message(InputMessage::blocks(Role::Assistant, message.content.clone()))
                        .message(InputMessage::blocks(
                            Role::User,
                            vec![ContentBlock::tool_result(id, "Sunny, 72°F")],
                        ))
                        .build(),
                )
                .await?;
            println!("{}", follow_up.text());
        }
    }
    Ok(())
}
```

## Count tokens

```rust
use simse::{Client, MessageCreateParams, types::InputMessage};

let count = client
    .messages()
    .count_tokens(
        MessageCreateParams::builder("zoysia", 1)
            .message(InputMessage::user("How many tokens is this?"))
            .build(),
    )
    .await?;
println!("{} input tokens", count.input_tokens);
```

## Models

```rust
let models = client.models().list().await?;
for m in &models.data {
    println!("{} — {}", m.id, m.display_name);
}

let zoysia = client.models().retrieve("zoysia").await?;
println!("{} max output: {:?}", zoysia.id, zoysia.max_tokens);
```

## Sessions (agentic loop)

Sessions are persisted, model-driven conversations where the agent reaches tools
through the orchestrator (distinct from the stateless Messages API).

```rust
use futures::StreamExt;
use simse::Client;
use simse::resource::Sessions;
use simse::params::SessionCreateParams;
use simse::types::SessionStreamEvent;

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    let session = client
        .sessions()
        .create(SessionCreateParams::new().title("Demo"))
        .await?;

    // Buffered prompt:
    let result = client.sessions().prompt(&session.id, "Summarize Rust ownership.").await?;
    println!("{}", result.message.content);

    // Streaming prompt:
    let mut stream = client.sessions().stream(&session.id, "Now in one sentence.").await?;
    while let Some(event) = stream.next().await {
        if let SessionStreamEvent::Delta { delta } = event? {
            print!("{delta}");
        }
    }

    client.sessions().delete(&session.id).await?;
    Ok(())
}
```

## Other surfaces

The full platform surface is covered:

```rust
client.account().retrieve().await?;            // GET /v1/account
client.usage().retrieve().await?;              // GET /v1/usage
client.billing().retrieve().await?;            // GET /v1/billing

client.memories().list().await?;               // GET /v1/memories
client.memories().create(params).await?;       // POST /v1/memories
client.memories().stats().await?;              // GET /v1/memories/stats

client.plugins().registry().await?;            // GET /v1/plugins/registry
client.plugins().install(install_params).await?;

client.pm().tasks().list(&session_id).await?;  // GET /v1/sessions/{id}/tasks
client.pm().projects().create(&session_id, p).await?;
client.pm().schedules().list().await?;         // GET /v1/schedules
client.pm().workflows().run(&wf_id, None, None).await?;

client.flags().get().await?;                   // GET /v1/flags
```

## Errors

Every API error is an `Error::Api(ApiErrorBody)` carrying the HTTP `status`, the
`request_id`, and the Anthropic `error.type`. Named predicates mirror the
Anthropic SDK classes:

```rust
match client.messages().create(params).await {
    Ok(message) => { /* ... */ }
    Err(e) if e.is_rate_limit() => eprintln!("rate limited; request-id {:?}", e.request_id()),
    Err(e) if e.is_authentication() => eprintln!("bad key"),
    Err(e) => eprintln!("{e}"),
}
```

The SDK parses **both** wire envelopes (the Anthropic
`{type,error:{type,message},request_id}` and the legacy `{error:{code,message}}`).

## Retries & rate limits

Requests retry on `408`, `409`, `429`, and `≥500` with exponential backoff,
honoring `retry-after` (default `max_retries = 2`). To surface the request id and
the `anthropic-ratelimit-*` budget on every response, register a hook:

```rust
use std::sync::Arc;

let client = Client::builder()
    .api_key("sk_...")
    .on_response(Arc::new(|meta| {
        if let Some(rid) = &meta.request_id {
            eprintln!("request-id: {rid}, remaining: {:?}", meta.requests_remaining);
        }
    }))
    .build()?;
```

Or use `messages().create_with_meta(...)` to get the `ResponseMeta` alongside the
`Message`.

## License

MIT — see [LICENSE](./LICENSE).
