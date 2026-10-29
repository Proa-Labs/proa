// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

//! OpenAPI is derived from the handlers and their actual request/response types.
//! Only cross-cutting HTTP middleware metadata and the deployment origin live here.

use crate::data::{routes, site};

use std::collections::BTreeMap;

use utoipa::{
    openapi::{
        external_docs::ExternalDocsBuilder, info::ContactBuilder, response::ResponseBuilder,
        Content, Header, OpenApi as Document, Ref, RefOr, Response, Server,
    },
    IntoResponses, OpenApi, PartialSchema, ToSchema,
};

use crate::endpoints::{agent_discovery, api, health, llms, search};
use crate::pages::home;

#[derive(OpenApi)]
#[openapi(
    info(
        title = "Proa public documentation API",
        version = "1.0.0",
        description = const_format::concatcp!("Public Proa Rust framework documentation. Search and retrieval are synchronous reads. Optional JSON exports run in background jobs: POST ", routes::API_EXPORTS, " returns 202 with Location and job_id; poll status_url, then retrieve result_url. No authentication is required. The sandbox server at ", routes::SANDBOX, " has isolated synthetic documents and the same versioned API contract for safe integration tests, with no credentials or production data. POST search supports optional Idempotency-Key response replay for fifteen minutes on the serving instance, with bounded capacity; replay and export job state are not shared between replicas or across restarts. All document reads are safe to repeat. The ", routes::API_V1, " prefix is the compatibility boundary: breaking changes receive a new prefix; deprecations are announced in ", routes::API_GUIDE, " before removal. Pagination cursors are opaque and valid only for the same result set and deployment. The complete website and legacy search contract is at ", routes::SITE_OPENAPI, ".")
    ),
    paths(
        api::search_handler,
        api::search_post_handler,
        api::list_handler,
        api::page_handler,
        api::exports::create,
        api::exports::status,
        api::exports::result,
        agent_discovery::site_metadata
    ),
    components(schemas(api::Problem))
)]
struct DocumentationApi;

/// Non-JSON content representations and legacy browser search have their own
/// contract. Tool generators can consume the v1 JSON API without accidentally
/// treating an HTML page, text corpus, or legacy scalar body as an agent tool.
#[derive(OpenApi)]
#[openapi(
    info(title = "Proa website and legacy interfaces", version = "1.0.0"),
    paths(
        api::search_handler,
        api::search_post_handler,
        api::list_handler,
        api::page_handler,
        api::exports::create,
        api::exports::status,
        api::exports::result,
        search::handler,
        search::post_handler,
        agent_discovery::site_metadata,
        agent_discovery::index,
        llms::site_handler,
        health::healthz,
        home::handler
    ),
    components(schemas(api::Problem))
)]
struct WebsiteApi;

impl IntoResponses for api::ApiError {
    fn responses() -> BTreeMap<String, RefOr<Response>> {
        [
            ("400", "Invalid parameters. Correct the query and retry."),
            ("404", "Unknown public document or API route."),
            ("405", "Unsupported HTTP method."),
            (
                "429",
                "Shared per-instance budget exhausted. Wait for Retry-After before retrying.",
            ),
        ]
        .into_iter()
        .map(|(status, description)| {
            let response = ResponseBuilder::new()
                .description(description)
                .content(
                    "application/problem+json",
                    Content::new(Some(Ref::from_schema_name(api::Problem::name()))),
                )
                .build();
            (status.into(), response.into())
        })
        .collect()
    }
}

fn header<T: PartialSchema>(description: &str) -> Header {
    let mut header = Header::new(T::schema());
    header.description = Some(description.into());
    header
}

fn rate_limit_headers() -> [(&'static str, Header); 5] {
    [
        (
            "RateLimit",
            header::<String>("Remaining shared per-instance budget and seconds until reset."),
        ),
        (
            "RateLimit-Policy",
            header::<String>("Shared documentation budget per sixty-second window."),
        ),
        (
            "RateLimit-Limit",
            header::<u32>("Requests per sixty-second window."),
        ),
        (
            "RateLimit-Remaining",
            header::<u32>("Requests remaining in this window."),
        ),
        (
            "RateLimit-Reset",
            header::<u64>("Seconds until the budget resets."),
        ),
    ]
}

pub fn document(origin: &str) -> Document {
    let mut document = configure(DocumentationApi::openapi(), origin);
    document.servers = Some(api_servers(origin));
    // Site metadata describes the production site; the sandbox's metadata has
    // its own explicitly synthetic shape and is not this operation.
    if let Some(item) = document.paths.paths.get_mut(routes::SITE_METADATA) {
        if let Some(operation) = &mut item.get {
            operation.servers = Some(vec![Server::new(origin)]);
        }
    }
    document
}

pub fn sandbox_document(origin: &str) -> Document {
    let mut document = document(origin);
    document.info.title = "Proa documentation API sandbox".into();
    document.servers = Some(vec![Server::new(format!("{origin}{}", routes::SANDBOX))]);
    document.paths.paths.remove(routes::SITE_METADATA);
    for item in document.paths.paths.values_mut() {
        for operation in [item.get.as_mut(), item.post.as_mut()]
            .into_iter()
            .flatten()
        {
            operation.servers = None;
        }
    }
    document
}

fn api_servers(origin: &str) -> Vec<Server> {
    let mut production = Server::new(origin);
    production.description = Some("Production: public documentation, no credentials.".into());
    let mut sandbox = Server::new(format!("{origin}{}", routes::SANDBOX));
    sandbox.description = Some("Sandbox / test environment: isolated synthetic fixture documents, no credentials or production data.".into());
    vec![production, sandbox]
}

pub fn website_document(origin: &str) -> Document {
    configure(WebsiteApi::openapi(), origin)
}

fn configure(mut document: Document, origin: &str) -> Document {
    document.servers = Some(vec![Server::new(origin)]);
    document.external_docs = Some(
        ExternalDocsBuilder::new()
            .url(format!("{origin}{}", routes::API_GUIDE))
            .description(Some("Documentation API and MCP usage guide."))
            .build(),
    );
    document.info.contact = Some(
        ContactBuilder::new()
            .name(Some(site::ORGANIZATION))
            .url(Some(format!("{origin}{}", routes::CONTACT)))
            .build(),
    );
    document.security = Some(Vec::new());

    // The same middleware wraps versioned reads and legacy browser search.
    // Describe its headers once, including on error responses.
    for (path, item) in &mut document.paths.paths {
        if !path.starts_with(routes::API_PREFIX) {
            continue;
        }
        for operation in [item.get.as_mut(), item.post.as_mut()]
            .into_iter()
            .flatten()
        {
            if path.starts_with(routes::API_V1) {
                operation.servers = Some(api_servers(origin));
            }
            for (status, response) in &mut operation.responses.responses {
                if let RefOr::T(response) = response {
                    response.headers.extend(
                        rate_limit_headers().map(|(name, value)| (name.into(), value.into())),
                    );
                    if status == "429" {
                        response.headers.insert(
                            "Retry-After".into(),
                            header::<u64>("Seconds to wait before retrying.").into(),
                        );
                    }
                }
            }
        }
    }
    document
}

#[cfg(test)]
mod tests {
    #[test]
    fn deployment_urls_are_serialized_without_json_injection() {
        let origin = "https://preview.example/\"</script>&";
        let document = serde_json::to_value(super::document(origin)).unwrap();
        assert_eq!(document["servers"][0]["url"], origin);
        assert_eq!(
            document["externalDocs"]["url"],
            format!("{origin}{}", super::routes::API_GUIDE)
        );
        assert_eq!(
            document["info"]["contact"]["url"],
            format!("{origin}{}", super::routes::CONTACT)
        );
    }
}
