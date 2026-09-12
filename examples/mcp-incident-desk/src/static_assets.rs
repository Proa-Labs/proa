// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use tower_http::services::ServeDir;

pub fn service() -> ServeDir {
    ServeDir::new("public").append_index_html_on_directories(false)
}
