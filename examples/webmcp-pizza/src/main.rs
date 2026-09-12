// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

use std::env;
use std::net::{IpAddr, SocketAddr};

use tracing_subscriber::EnvFilter;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(EnvFilter::try_from_default_env().unwrap_or_else(|_| "info".into()))
        .init();

    let host = env::var("HOST").unwrap_or_else(|_| "127.0.0.1".to_owned());
    let port = env::var("PORT")
        .ok()
        .and_then(|value| value.parse::<u16>().ok())
        .unwrap_or(5560);
    let address = SocketAddr::new(host.parse::<IpAddr>()?, port);
    let listener = tokio::net::TcpListener::bind(address).await?;

    tracing::info!("pizza demo listening on http://{}", listener.local_addr()?);
    axum::serve(listener, proa_webmcp_pizza::app()).await?;
    Ok(())
}
