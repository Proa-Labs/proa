// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use serde::{Deserialize, Serialize};

/// Streaming/rendering mode for a request.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RenderMode {
    ChunkedInOrder,
    ChunkedOutOfOrder,
    #[default]
    Static,
    Agent,
}

impl RenderMode {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::ChunkedInOrder => "chunked_in_order",
            Self::ChunkedOutOfOrder => "chunked_out_of_order",
            Self::Static => "static",
            Self::Agent => "agent",
        }
    }

    pub fn from_header(value: &str) -> Option<Self> {
        Self::parse(value)
    }

    pub fn from_query_param(value: &str) -> Option<Self> {
        Self::parse(value)
    }

    fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "chunked_in_order" | "chunked-in-order" | "in_order" | "in-order" => {
                Some(Self::ChunkedInOrder)
            }
            "chunked_out_of_order" | "chunked-out-of-order" | "out_of_order" | "out-of-order" => {
                Some(Self::ChunkedOutOfOrder)
            }
            "static" => Some(Self::Static),
            "agent" => Some(Self::Agent),
            _ => None,
        }
    }
}
