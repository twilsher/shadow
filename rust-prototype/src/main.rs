mod config;
mod logger;
mod proxy;

use axum::{routing::any, Router};
use std::path::PathBuf;
use tokio::signal;
use tracing::info;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt};

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::registry()
        .with(
            tracing_subscriber::EnvFilter::try_from_default_env()
                .unwrap_or_else(|_| "shadow_proxy=info,tower_http=debug".into()),
        )
        .with(tracing_subscriber::fmt::layer().with_target(true))
        .init();

    info!("Shadow Proxy starting...");

    // Load configuration
    let config = config::Config::from_env()?;
    config.validate()?;

    info!(
        proxy_addr = %config.proxy.address,
        old_servers = ?config.proxy.old_servers,
        new_servers = ?config.proxy.new_servers,
        "Configuration loaded"
    );

    // Create proxy state and result channel
    let (proxy_state, result_rx) = proxy::ProxyState::new(config.proxy.clone());

    // Start result logger
    let log_path = std::env::var("SHADOW_LOG_FILE")
        .ok()
        .map(PathBuf::from)
        .or(Some(PathBuf::from("log/shadow-results.log")));

    let logger = logger::ResultLogger::new(log_path)?;
    let logger_handle = tokio::spawn(logger.run(result_rx));

    // Create router
    let app = Router::new()
        .route("/*path", any(proxy::proxy_handler))
        .route("/", any(proxy::proxy_handler))
        .with_state(proxy_state.clone());

    // Create server
    let listener = tokio::net::TcpListener::bind(config.proxy.address).await?;
    info!("Proxy listening on {}", config.proxy.address);

    info!("Shadow Proxy started successfully");
    info!("Send requests to http://{}", config.proxy.address);
    info!("Logs will be written to {:?}", std::env::var("SHADOW_LOG_FILE").ok());

    // Run server with graceful shutdown
    axum::serve(listener, app)
        .with_graceful_shutdown(shutdown_signal())
        .await?;

    info!("Proxy server stopped, waiting for logger to finish...");
    drop(proxy_state.result_tx);
    logger_handle.await?;

    info!("Shadow Proxy stopped");
    Ok(())
}

async fn shutdown_signal() {
    let ctrl_c = async {
        signal::ctrl_c()
            .await
            .expect("failed to install Ctrl+C handler");
    };

    #[cfg(unix)]
    let terminate = async {
        signal::unix::signal(signal::unix::SignalKind::terminate())
            .expect("failed to install signal handler")
            .recv()
            .await;
    };

    #[cfg(not(unix))]
    let terminate = std::future::pending::<()>();

    tokio::select! {
        _ = ctrl_c => {
            info!("Received Ctrl+C signal");
        },
        _ = terminate => {
            info!("Received terminate signal");
        },
    }
}
