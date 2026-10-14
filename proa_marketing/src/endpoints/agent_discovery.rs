// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use axum::http::header;

pub const DISCOVERY_LINKS: &str = concat!(
    "</.well-known/proa-site.json>; rel=\"service-meta\"; type=\"application/json\", ",
    "</openapi.json>; rel=\"service-desc\"; ",
    "type=\"application/vnd.oai.openapi+json\""
);

const SITE_METADATA: &str = r#"{
  "format": "proa-site",
  "version": "0.1",
  "status": "experimental",
  "site": {
    "name": "Proa",
    "description": "An extremely performant, batteries-included Rust web framework built around a new programming model for composable, multi-target rendering.",
    "homepage": "/"
  },
  "interfaces": [
    {
      "kind": "agent-instructions",
      "url": "/llms-full.txt",
      "media_type": "text/plain"
    },
    {
      "kind": "openapi",
      "url": "/openapi.json",
      "media_type": "application/vnd.oai.openapi+json"
    }
  ],
  "authentication": {
    "required": false
  }
}"#;

const OPENAPI: &str = r#"{
  "openapi": "3.1.0",
  "info": {
    "title": "Proa public site API",
    "version": "0.1.0",
    "description": "Read-only, unauthenticated interfaces exposed by the Proa marketing site."
  },
  "servers": [
    {
      "url": "/"
    }
  ],
  "paths": {
    "/": {
      "get": {
        "operationId": "getHomePage",
        "summary": "Fetch the server-rendered Proa home page",
        "responses": {
          "200": {
            "description": "Server-rendered HTML"
          }
        }
      }
    },
    "/.well-known/proa-site.json": {
      "get": {
        "operationId": "getProaSiteMetadata",
        "summary": "Describe the machine-readable interfaces exposed by this site",
        "responses": {
          "200": {
            "description": "Proa site metadata"
          }
        }
      }
    },
    "/llms-full.txt": {
      "get": {
        "operationId": "getDocumentationCorpus",
        "summary": "Fetch the Proa documentation corpus as plain text",
        "responses": {
          "200": {
            "description": "Plain-text documentation corpus"
          }
        }
      }
    },
    "/api/search": {
      "get": {
        "operationId": "searchProaDocumentation",
        "summary": "Search the Proa documentation",
        "parameters": [
          {
            "name": "q",
            "in": "query",
            "required": true,
            "schema": {
              "type": "string"
            }
          }
        ],
        "responses": {
          "200": {
            "description": "Matching documentation pages"
          }
        }
      }
    },
    "/healthz": {
      "get": {
        "operationId": "getHealth",
        "summary": "Check whether the site process is healthy",
        "responses": {
          "200": {
            "description": "Healthy"
          }
        }
      }
    }
  }
}"#;

pub async fn site_metadata() -> impl axum::response::IntoResponse {
    (
        [
            (header::CONTENT_TYPE, "application/json; charset=utf-8"),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        SITE_METADATA,
    )
}

pub async fn openapi() -> impl axum::response::IntoResponse {
    (
        [
            (
                header::CONTENT_TYPE,
                "application/vnd.oai.openapi+json; charset=utf-8",
            ),
            (header::CACHE_CONTROL, "public, max-age=300"),
        ],
        OPENAPI,
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn site_metadata_is_valid_and_points_to_live_interfaces() {
        let metadata: serde_json::Value = serde_json::from_str(SITE_METADATA).unwrap();
        assert_eq!(metadata["format"], "proa-site");
        assert_eq!(metadata["status"], "experimental");

        let interfaces = metadata["interfaces"].as_array().unwrap();
        assert!(interfaces
            .iter()
            .any(|interface| interface["url"] == "/llms-full.txt"));
        assert!(interfaces
            .iter()
            .any(|interface| interface["url"] == "/openapi.json"));
    }

    #[test]
    fn openapi_document_is_valid_json_and_describes_search() {
        let openapi: serde_json::Value = serde_json::from_str(OPENAPI).unwrap();
        assert_eq!(openapi["openapi"], "3.1.0");
        assert_eq!(
            openapi["paths"]["/api/search"]["get"]["operationId"],
            "searchProaDocumentation"
        );
    }
}
