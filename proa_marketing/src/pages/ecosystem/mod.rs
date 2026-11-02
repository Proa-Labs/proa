// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use crate::data::routes;

use crate::utils::document::Url;
use axum::response::Html;
use proa_core::{DataLoader, MdRenderSync, WebContext, WriteBuf, WriteError};
use proa_macros::md_sync;

mod apps;
mod data;
mod developer_tools;
mod layout;
mod registry;

pub async fn handler() -> Html<String> {
    apps::render(routes::ECOSYSTEM)
}

pub async fn apps_handler() -> Html<String> {
    apps::render(routes::APPS)
}

pub async fn developer_tools_handler() -> Html<String> {
    developer_tools::render()
}

pub async fn registry_handler() -> Html<String> {
    registry::render()
}

/// The index uses the same application and tool records as the HTML catalog.
pub(crate) struct EcosystemIndex;

impl<L: DataLoader, B: WriteBuf> MdRenderSync<L, B> for EcosystemIndex {
    fn render_md(self, cx: &mut WebContext<L, B>) -> Result<(), WriteError> {
        let data = data::ecosystem_data();
        md_sync! { r###"
            # Proa ecosystem

            > Applications, reference showcases, and developer tools built with Proa.

            ## Applications and showcases

            {for app in data.apps {
                md_sync! { r#"
                    - [{app.name}]({Url(app.href)}): {app.product_kind}
                    "#
                }
            }}

            ## Developer tools

            {for tool in data.tools {
                md_sync! { r#"
                    - [{tool.name}]({Url(tool.href)}): {tool.description}
                    "#
                }
            }}"###
        }
        .render_md(cx)
    }
}
