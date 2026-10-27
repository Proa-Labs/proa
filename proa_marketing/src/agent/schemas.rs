// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use const_json::const_json;

pub const SEARCH_INPUT: &str = const_json!({
    "type": "object",
    "properties": {
        "q": {"type": "string", "minLength": 1, "maxLength": 256, "description": "Phrase to find in document titles, descriptions, or Markdown."},
        "limit": {"type": "integer", "minimum": 1, "maximum": 50, "description": "Maximum results per page; defaults to 10."},
        "cursor": {"type": "string", "description": "next_cursor from a preceding search with the same q."}
    },
    "required": ["q"],
    "additionalProperties": false
});

pub const GET_INPUT: &str = const_json!({
    "type": "object",
    "properties": {
        "path": {"type": "string", "description": "Published route or Markdown path, for example /docs/core/html-rendering."}
    },
    "required": ["path"],
    "additionalProperties": false
});
