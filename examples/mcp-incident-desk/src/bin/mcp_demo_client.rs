// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use mcp_incident_desk::client::{DesktopBridge, OAuthClient};
use rmcp::{model::CallToolRequestParams, ServiceExt};
#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let bridge = args.iter().any(|v| v == "--stdio");
    let write = args.iter().any(|v| v == "--write");
    let resource = std::env::var("MCP_URL").unwrap_or_else(|_| "https://localhost:5570/mcp".into());
    let ca = std::env::var("MCP_CA").unwrap_or_else(|_| ".local/ca.pem".into());
    let client = OAuthClient::new(resource, Some(std::path::Path::new(&ca)), bridge, write)?;
    if bridge {
        // Desktop clients cancel a tool call well before the 300s authorization
        // window closes. That abort would drop `authorize()` mid-flight and open
        // a second browser tab on the next call. Warm credentials while the
        // desktop client is still initializing so the first tool call finds them.
        // Set INCIDENT_DESK_EAGER_AUTH=0 to keep authorization lazy, which lets a
        // protocol-only check exercise the handshake without opening a browser.
        if std::env::var("INCIDENT_DESK_EAGER_AUTH").as_deref() != Ok("0") {
            let warm = client.clone();
            tokio::spawn(async move {
                if let Err(err) = warm.token().await {
                    eprintln!("Incident Desk: initial authorization failed: {err:#}");
                }
            });
        }
        DesktopBridge { client }
            .serve(rmcp::transport::stdio())
            .await?
            .waiting()
            .await?;
    } else {
        let upstream = client.connect().await?;
        let tools = upstream.list_all_tools().await?;
        println!("Connected. {} tools available.", tools.len());
        for (name, args) in [
            ("whoami", serde_json::json!({})),
            (
                "get_issue_brief",
                serde_json::json!({"issue_id":"AST-1042"}),
            ),
            ("get_issue_data", serde_json::json!({"issue_id":"AST-1042"})),
        ] {
            let result = upstream
                .call_tool(
                    CallToolRequestParams::new(name)
                        .with_arguments(args.as_object().unwrap().clone()),
                )
                .await?;
            println!("{}", serde_json::to_string_pretty(&result)?);
        }
        upstream.cancel().await?;
    }
    Ok(())
}
