use axum::{
    body::Body,
    extract::State,
    http::{HeaderMap, Method, StatusCode, Uri},
    response::{IntoResponse, Response},
};
use chrono::Utc;
use futures::future::join_all;
use reqwest::Client;
use serde::{Deserialize, Serialize};
use std::{sync::Arc, time::Duration};
use tokio::sync::mpsc;
use tracing::{error, info, warn};

use crate::config::ProxyConfig;

#[derive(Clone)]
pub struct ProxyState {
    pub config: Arc<ProxyConfig>,
    pub client: Client,
    pub result_tx: mpsc::UnboundedSender<ProxyResult>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyResult {
    pub timestamp: String,
    pub request: RequestInfo,
    pub old_responses: Vec<ResponseInfo>,
    pub new_responses: Vec<ResponseInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestInfo {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ResponseInfo {
    pub server: String,
    pub status: u16,
    pub elapsed_ms: u64,
    pub success: bool,
    pub error: Option<String>,
    pub body_preview: String,
}

impl ProxyState {
    pub fn new(config: ProxyConfig) -> (Self, mpsc::UnboundedReceiver<ProxyResult>) {
        let (result_tx, result_rx) = mpsc::unbounded_channel();

        let client = Client::builder()
            .pool_max_idle_per_host(20)
            .pool_idle_timeout(Some(Duration::from_secs(60)))
            .timeout(Duration::from_secs(config.old_servers_timeout_secs.max(config.new_servers_timeout_secs)))
            .build()
            .expect("Failed to create HTTP client");

        let state = Self {
            config: Arc::new(config),
            client,
            result_tx,
        };

        (state, result_rx)
    }
}

pub async fn proxy_handler(
    State(state): State<ProxyState>,
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    body: Body,
) -> Response {
    let start = std::time::Instant::now();

    // Extract path and query
    let path = uri.path();
    let query = uri.query().unwrap_or("");
    let full_path = if query.is_empty() {
        path.to_string()
    } else {
        format!("{}?{}", path, query)
    };

    info!(
        method = %method,
        path = %full_path,
        "Proxying request"
    );

    // Convert body to bytes (we need to clone it for multiple requests)
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => bytes,
        Err(e) => {
            error!(error = %e, "Failed to read request body");
            return (
                StatusCode::BAD_REQUEST,
                "Failed to read request body",
            ).into_response();
        }
    };

    // Prepare request info for logging
    let request_info = RequestInfo {
        method: method.to_string(),
        path: full_path.clone(),
        headers: headers
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_str().unwrap_or("").to_string()))
            .collect(),
    };

    // Forward to old servers
    let old_futures: Vec<_> = state
        .config
        .old_servers
        .iter()
        .map(|server| {
            let state = state.clone();
            let method = method.clone();
            let path = full_path.clone();
            let headers = headers.clone();
            let body = body_bytes.clone();
            let server = server.clone();

            tokio::spawn(async move {
                forward_request(
                    &state,
                    &server,
                    &method,
                    &path,
                    &headers,
                    body,
                    state.config.old_servers_timeout_secs,
                    &state.config.old_servers_additional_headers,
                )
                .await
            })
        })
        .collect();

    // Forward to new servers
    let new_futures: Vec<_> = state
        .config
        .new_servers
        .iter()
        .map(|server| {
            let state = state.clone();
            let method = method.clone();
            let path = full_path.clone();
            let headers = headers.clone();
            let body = body_bytes.clone();
            let server = server.clone();

            tokio::spawn(async move {
                forward_request(
                    &state,
                    &server,
                    &method,
                    &path,
                    &headers,
                    body,
                    state.config.new_servers_timeout_secs,
                    &state.config.new_servers_additional_headers,
                )
                .await
            })
        })
        .collect();

    // Wait for all responses
    let old_results = join_all(old_futures).await;
    let new_results = join_all(new_futures).await;

    let old_responses: Vec<ResponseInfo> = old_results
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    let new_responses: Vec<ResponseInfo> = new_results
        .into_iter()
        .filter_map(|r| r.ok())
        .collect();

    // Log the result
    let result = ProxyResult {
        timestamp: Utc::now().to_rfc3339(),
        request: request_info,
        old_responses: old_responses.clone(),
        new_responses: new_responses.clone(),
    };

    if let Err(e) = state.result_tx.send(result) {
        error!(error = %e, "Failed to send proxy result");
    }

    let elapsed = start.elapsed();
    info!(
        elapsed_ms = elapsed.as_millis(),
        old_servers = old_responses.len(),
        new_servers = new_responses.len(),
        "Request completed"
    );

    // Return the first old server response or error
    match old_responses.first() {
        Some(resp) if resp.success => {
            let status = StatusCode::from_u16(resp.status)
                .unwrap_or(StatusCode::INTERNAL_SERVER_ERROR);
            (status, resp.body_preview.clone()).into_response()
        }
        _ => {
            warn!("All old servers failed or returned errors");
            (
                StatusCode::BAD_GATEWAY,
                "Upstream old server error",
            ).into_response()
        }
    }
}

async fn forward_request(
    state: &ProxyState,
    server: &str,
    method: &Method,
    path: &str,
    headers: &HeaderMap,
    body: bytes::Bytes,
    timeout_secs: u64,
    additional_headers: &[(String, String)],
) -> ResponseInfo {
    let start = std::time::Instant::now();
    let url = format!("{}{}", server.trim_end_matches('/'), path);

    // Build request
    let mut req = state.client.request(method.clone(), &url);

    // Copy headers (excluding problematic ones)
    for (key, value) in headers.iter() {
        let key_str = key.as_str();
        if !matches!(
            key_str.to_lowercase().as_str(),
            "host" | "content-length" | "transfer-encoding"
        ) {
            if let Ok(val_str) = value.to_str() {
                req = req.header(key_str, val_str);
            }
        }
    }

    // Add additional headers
    for (key, value) in additional_headers {
        req = req.header(key, value);
    }

    // Add body if not empty
    if !body.is_empty() {
        req = req.body(body);
    }

    // Set timeout
    req = req.timeout(Duration::from_secs(timeout_secs));

    // Send request
    match req.send().await {
        Ok(response) => {
            let status = response.status().as_u16();
            let elapsed = start.elapsed();

            match response.text().await {
                Ok(body) => {
                    let body_preview = if body.len() > 1000 {
                        format!("{}... (truncated)", &body[..1000])
                    } else {
                        body
                    };

                    ResponseInfo {
                        server: server.to_string(),
                        status,
                        elapsed_ms: elapsed.as_millis() as u64,
                        success: true,
                        error: None,
                        body_preview,
                    }
                }
                Err(e) => {
                    error!(server = %server, error = %e, "Failed to read response body");
                    ResponseInfo {
                        server: server.to_string(),
                        status,
                        elapsed_ms: elapsed.as_millis() as u64,
                        success: false,
                        error: Some(format!("Failed to read response body: {}", e)),
                        body_preview: String::new(),
                    }
                }
            }
        }
        Err(e) => {
            let elapsed = start.elapsed();
            error!(server = %server, error = %e, "Request failed");

            ResponseInfo {
                server: server.to_string(),
                status: 0,
                elapsed_ms: elapsed.as_millis() as u64,
                success: false,
                error: Some(e.to_string()),
                body_preview: String::new(),
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_request_info_serialization() {
        let info = RequestInfo {
            method: "GET".to_string(),
            path: "/test".to_string(),
            headers: vec![("X-Test".to_string(), "value".to_string())],
        };

        let json = serde_json::to_string(&info).unwrap();
        assert!(json.contains("GET"));
        assert!(json.contains("/test"));
    }

    #[test]
    fn test_response_info_success() {
        let info = ResponseInfo {
            server: "http://localhost:8080".to_string(),
            status: 200,
            elapsed_ms: 42,
            success: true,
            error: None,
            body_preview: "OK".to_string(),
        };

        assert!(info.success);
        assert!(info.error.is_none());
    }

    #[test]
    fn test_response_info_error() {
        let info = ResponseInfo {
            server: "http://localhost:8080".to_string(),
            status: 0,
            elapsed_ms: 100,
            success: false,
            error: Some("Connection refused".to_string()),
            body_preview: String::new(),
        };

        assert!(!info.success);
        assert!(info.error.is_some());
    }
}
