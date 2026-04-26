use crate::proxy::ProxyResult;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use tokio::sync::mpsc;
use tracing::{error, info};

pub struct ResultLogger {
    log_file: Option<File>,
}

impl ResultLogger {
    pub fn new(log_path: Option<PathBuf>) -> anyhow::Result<Self> {
        let log_file = if let Some(path) = log_path {
            // Create parent directory if it doesn't exist
            if let Some(parent) = path.parent() {
                std::fs::create_dir_all(parent)?;
            }

            Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(path)?,
            )
        } else {
            None
        };

        Ok(Self { log_file })
    }

    pub async fn run(mut self, mut result_rx: mpsc::UnboundedReceiver<ProxyResult>) {
        info!("Result logger started");

        while let Some(result) = result_rx.recv().await {
            self.log_result(&result);
        }

        info!("Result logger stopped");
    }

    fn log_result(&mut self, result: &ProxyResult) {
        // Log to file if configured
        if let Some(ref mut file) = self.log_file {
            match serde_json::to_string(result) {
                Ok(json) => {
                    if let Err(e) = writeln!(file, "{}", json) {
                        error!(error = %e, "Failed to write to log file");
                    }
                }
                Err(e) => {
                    error!(error = %e, "Failed to serialize result");
                }
            }
        }

        // Log summary to console
        let old_success = result
            .old_responses
            .iter()
            .filter(|r| r.success)
            .count();
        let new_success = result
            .new_responses
            .iter()
            .filter(|r| r.success)
            .count();

        info!(
            method = %result.request.method,
            path = %result.request.path,
            old_success = old_success,
            old_total = result.old_responses.len(),
            new_success = new_success,
            new_total = result.new_responses.len(),
            "Request completed"
        );

        // Log differences in status codes
        let old_statuses: Vec<u16> = result
            .old_responses
            .iter()
            .filter(|r| r.success)
            .map(|r| r.status)
            .collect();

        let new_statuses: Vec<u16> = result
            .new_responses
            .iter()
            .filter(|r| r.success)
            .map(|r| r.status)
            .collect();

        if !old_statuses.is_empty() && !new_statuses.is_empty() {
            let old_avg = old_statuses.iter().sum::<u16>() as f64 / old_statuses.len() as f64;
            let new_avg = new_statuses.iter().sum::<u16>() as f64 / new_statuses.len() as f64;

            if (old_avg - new_avg).abs() > 0.1 {
                info!(
                    old_status = ?old_statuses,
                    new_status = ?new_statuses,
                    "Status code difference detected"
                );
            }
        }

        // Log timing differences
        let old_times: Vec<u64> = result
            .old_responses
            .iter()
            .filter(|r| r.success)
            .map(|r| r.elapsed_ms)
            .collect();

        let new_times: Vec<u64> = result
            .new_responses
            .iter()
            .filter(|r| r.success)
            .map(|r| r.elapsed_ms)
            .collect();

        if !old_times.is_empty() && !new_times.is_empty() {
            let old_avg = old_times.iter().sum::<u64>() as f64 / old_times.len() as f64;
            let new_avg = new_times.iter().sum::<u64>() as f64 / new_times.len() as f64;

            info!(
                old_avg_ms = old_avg,
                new_avg_ms = new_avg,
                diff_ms = new_avg - old_avg,
                "Response timing"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proxy::{RequestInfo, ResponseInfo};

    #[test]
    fn test_logger_creation_without_file() {
        let logger = ResultLogger::new(None);
        assert!(logger.is_ok());
    }

    #[test]
    fn test_log_result_doesnt_panic() {
        let mut logger = ResultLogger::new(None).unwrap();

        let result = ProxyResult {
            timestamp: "2024-01-01T00:00:00Z".to_string(),
            request: RequestInfo {
                method: "GET".to_string(),
                path: "/test".to_string(),
                headers: vec![],
            },
            old_responses: vec![ResponseInfo {
                server: "http://localhost:8080".to_string(),
                status: 200,
                elapsed_ms: 10,
                success: true,
                error: None,
                body_preview: "OK".to_string(),
            }],
            new_responses: vec![ResponseInfo {
                server: "http://localhost:8082".to_string(),
                status: 200,
                elapsed_ms: 12,
                success: true,
                error: None,
                body_preview: "OK".to_string(),
            }],
        };

        logger.log_result(&result);
    }
}
