//! Example: Connectors API — register an MCP server, probe it, list, and clean up.
//!
//! ```no_run
//! cargo run --example connectors
//! ```
//!
//! Reads `SIMSE_API_KEY` from the environment.

use simse::{Client, ConnectorAuth, ConnectorCreateParams};

#[tokio::main]
async fn main() -> simse::Result<()> {
    let client = Client::from_env()?;

    // Register a remote-MCP connector with a static bearer token.
    let connector = client
        .connectors()
        .create(
            ConnectorCreateParams::new(
                "My MCP Server",
                "https://my-mcp-server.example.com/mcp",
                ConnectorAuth::bearer("bearer_token_here"),
            )
            .tool_denylist(vec!["admin_tool".to_string()]),
        )
        .await?;
    println!("Created connector: {}", connector.id);

    // Probe the server — live tools/list.
    let result = client.connectors().test(&connector.id).await?;
    if result.ok {
        println!("Connected — {} tool(s) available", result.tool_count);
    } else {
        println!("Connection failed: {:?}", result.error);
    }

    // List all connectors (secrets redacted).
    let list = client.connectors().list().await?;
    println!("Total connectors: {}", list.connectors.len());

    // Delete the connector.
    client.connectors().delete(&connector.id).await?;
    println!("Deleted connector {}", connector.id);

    Ok(())
}
