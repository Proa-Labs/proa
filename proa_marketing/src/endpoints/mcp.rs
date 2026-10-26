// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! Stateless documentation MCP backed by the same registry as the REST API.

use std::sync::{Arc, OnceLock};

use rmcp::transport::streamable_http_server::{
    session::local::LocalSessionManager, StreamableHttpServerConfig, StreamableHttpService,
};
use rmcp::{model::*, service::RequestContext, ErrorData as McpError, RoleServer, ServerHandler};
use serde_json::{json, Value};

use super::api;
use crate::utils::site_url::public_site_url;

#[derive(Clone)]
pub struct DocumentationServer;

fn tools() -> &'static [Tool] {
    static TOOLS: OnceLock<Vec<Tool>> = OnceLock::new();
    TOOLS.get_or_init(|| {
        let annotations = ToolAnnotations::new().read_only(true).destructive(false).idempotent(true).open_world(false);
        vec![
            Tool::new("search_docs", "Search official Proa Rust framework documentation by a case-insensitive phrase. Use this to find current APIs, render patterns, RSJS guidance, and deployment instructions. Returns titles, routes, Markdown URLs, and a pagination cursor; call get_doc with a returned path for the complete page.",
                json!({"type":"object","properties":{
                    "q":{"type":"string","minLength":1,"maxLength":256,"description":"Phrase to find in document titles, descriptions, or Markdown."},
                    "limit":{"type":"integer","minimum":1,"maximum":50,"description":"Maximum results per page; defaults to 10."},
                    "cursor":{"type":"string","description":"next_cursor from a preceding search with the same q."}
                },"required":["q"],"additionalProperties":false}).as_object().unwrap().clone())
                .with_raw_output_schema(output_schema("DocumentList"))
                .with_annotations(annotations.clone()),
            Tool::new("get_doc", "Read a complete official Proa documentation page as Markdown with its title, description, and canonical route. Supply a path returned by search_docs or resources/list. Only published documentation is accessible; arbitrary URLs and filesystem paths are not fetched.",
                json!({"type":"object","properties":{
                    "path":{"type":"string","description":"Published route or Markdown path, for example /docs/core/html-rendering."}
                },"required":["path"],"additionalProperties":false}).as_object().unwrap().clone())
                .with_raw_output_schema(output_schema("Document"))
                .with_annotations(annotations),
        ]
    })
}

/// Reuse the public output contract, resolving refs within the tool's schema.
fn output_schema(name: &str) -> Arc<JsonObject> {
    let spec: Value = serde_json::from_str(
        &include_str!("../agent/openapi.json").replace("#/components/schemas/", "#/$defs/"),
    )
    .unwrap();
    let mut schema = spec["components"]["schemas"][name].clone();
    schema["type"] = json!("object");
    schema["$defs"] = spec["components"]["schemas"].clone();
    Arc::new(schema.as_object().unwrap().clone())
}

impl ServerHandler for DocumentationServer {
    fn get_info(&self) -> ServerInfo {
        ServerInfo::new(ServerCapabilities::builder().enable_tools().enable_resources().build())
            .with_server_info(Implementation::new("proa-docs", "1.0.0").with_website_url(format!("{}/docs/api", public_site_url())))
            .with_instructions("Public Proa Rust framework documentation. Search for a focused phrase, then read the returned page. All tools and resources are read-only and require no credentials. Cite the returned canonical documentation paths. Pagination cursors apply to this deployment; restart after an update.")
    }

    async fn list_tools(
        &self,
        _: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListToolsResult, McpError> {
        Ok(ListToolsResult {
            tools: tools().to_vec(),
            ..Default::default()
        })
    }

    fn get_tool(&self, name: &str) -> Option<Tool> {
        tools().iter().find(|tool| tool.name == name).cloned()
    }

    async fn call_tool(
        &self,
        request: CallToolRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<CallToolResponse, McpError> {
        let args = Value::Object(request.arguments.unwrap_or_default());
        let result = match request.name.as_ref() {
            "search_docs" => serde_json::from_value(args)
                .map_err(|_| {
                    api::ApiError::invalid(
                        "Expected q (string), optional limit (integer), and cursor (string).",
                    )
                })
                .and_then(api::search)
                .map(|result| serde_json::to_value(result).unwrap()),
            "get_doc" => serde_json::from_value(args)
                .map_err(|_| {
                    api::ApiError::invalid(
                        "Expected a path string from search_docs or resources/list.",
                    )
                })
                .and_then(api::get)
                .map(|result| serde_json::to_value(result).unwrap()),
            _ => {
                return Err(McpError::invalid_params(
                    "Unknown tool. Use tools/list to discover available tools.",
                    None,
                ))
            }
        };
        Ok(match result {
            Ok(result) => CallToolResult::structured(result),
            Err(error) => CallToolResult::error(vec![ContentBlock::text(format!(
                "{}: {}",
                error.code, error.detail
            ))]),
        }
        .into())
    }

    async fn list_resources(
        &self,
        params: Option<PaginatedRequestParams>,
        _: RequestContext<RoleServer>,
    ) -> Result<ListResourcesResult, McpError> {
        let page = api::list(api::Pagination {
            limit: Some(50),
            cursor: params.and_then(|p| p.cursor),
        })
        .map_err(|error| McpError::invalid_params(error.detail, None))?;
        let resources = page
            .results
            .into_iter()
            .map(|doc| {
                let page = api::page(doc.path).expect("listed public document");
                Resource::new(
                    format!("{}{}", public_site_url(), doc.markdown_url),
                    doc.path,
                )
                .with_title(doc.title)
                .with_description(doc.description)
                .with_mime_type("text/markdown")
                .with_size(page.markdown.len() as u64)
            })
            .collect();
        Ok(ListResourcesResult {
            resources,
            next_cursor: page.next_cursor,
            ..Default::default()
        })
    }

    async fn read_resource(
        &self,
        request: ReadResourceRequestParams,
        _: RequestContext<RoleServer>,
    ) -> Result<ReadResourceResponse, McpError> {
        let path = request
            .uri
            .strip_prefix(public_site_url())
            .filter(|path| path.starts_with('/'))
            .ok_or_else(|| {
                McpError::invalid_params(
                    "Use a documentation URI returned by resources/list.",
                    None,
                )
            })?;
        let page = api::page(path).map_err(|error| McpError::invalid_params(error.detail, None))?;
        Ok(ReadResourceResult::new(vec![
            ResourceContents::text(page.markdown, request.uri).with_mime_type("text/markdown")
        ])
        .into())
    }
}

pub fn service() -> StreamableHttpService<DocumentationServer, LocalSessionManager> {
    let origin = public_site_url();
    let authority = origin
        .split_once("://")
        .map(|(_, authority)| authority)
        .unwrap_or(origin);
    let config = StreamableHttpServerConfig::default()
        .with_legacy_session_mode(false)
        .with_json_response(true)
        .with_max_request_body_bytes(16 * 1024)
        .with_allowed_hosts([authority, "localhost", "127.0.0.1", "[::1]"])
        .with_allowed_origins([origin]);
    StreamableHttpService::new(
        || Ok(DocumentationServer),
        Arc::new(LocalSessionManager::default()),
        config,
    )
}
