// Copyright Proa Labs, Inc.
// SPDX-License-Identifier: Apache-2.0

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "__CRATE_IDENT__=info,tower_http=info".into()),
        )
        .init();

    let host = std::env::var("PROA_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
    let port = std::env::var("PORT").unwrap_or_else(|_| "3000".to_string());
    let address: std::net::SocketAddr = format!("{host}:{port}").parse()?;
    let listener = tokio::net::TcpListener::bind(address).await?;
    let base_path = proa_core::BasePath::new(&std::env::var("PROA_BASE_PATH").unwrap_or_default())?;
    tracing::info!(address = %listener.local_addr()?, "ORIGIN listening");
    let _ = proa_framework_axum::serve(
        listener,
        __CRATE_IDENT__::app_with_base_path(base_path),
        std::time::Duration::from_secs(30),
    )
    .await?;
    Ok(())
}
