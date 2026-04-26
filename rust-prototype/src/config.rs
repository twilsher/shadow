use serde::{Deserialize, Serialize};
use std::net::SocketAddr;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub proxy: ProxyConfig,
    pub ui: UiConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyConfig {
    #[serde(default = "default_proxy_addr")]
    pub address: SocketAddr,

    pub old_servers: Vec<String>,
    pub new_servers: Vec<String>,

    #[serde(default = "default_timeout")]
    pub old_servers_timeout_secs: u64,

    #[serde(default = "default_timeout")]
    pub new_servers_timeout_secs: u64,

    #[serde(default)]
    pub old_servers_additional_headers: Vec<(String, String)>,

    #[serde(default)]
    pub new_servers_additional_headers: Vec<(String, String)>,

    #[serde(default = "default_max_concurrent")]
    pub max_concurrent_requests: usize,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UiConfig {
    #[serde(default = "default_ui_addr")]
    pub address: SocketAddr,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let proxy_addr = std::env::var("SHADOW_PROXY_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:8081".to_string())
            .parse()?;

        let ui_addr = std::env::var("SHADOW_UI_ADDR")
            .unwrap_or_else(|_| "0.0.0.0:9000".to_string())
            .parse()?;

        let old_servers = std::env::var("SHADOW_OLD_SERVERS")
            .unwrap_or_else(|_| "http://localhost:8080".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let new_servers = std::env::var("SHADOW_NEW_SERVERS")
            .unwrap_or_else(|_| "http://localhost:8082".to_string())
            .split(',')
            .map(|s| s.trim().to_string())
            .filter(|s| !s.is_empty())
            .collect();

        let old_timeout = std::env::var("SHADOW_OLD_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15);

        let new_timeout = std::env::var("SHADOW_NEW_TIMEOUT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(15);

        let max_concurrent = std::env::var("SHADOW_MAX_CONCURRENT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(1000);

        Ok(Config {
            proxy: ProxyConfig {
                address: proxy_addr,
                old_servers,
                new_servers,
                old_servers_timeout_secs: old_timeout,
                new_servers_timeout_secs: new_timeout,
                old_servers_additional_headers: vec![],
                new_servers_additional_headers: vec![],
                max_concurrent_requests: max_concurrent,
            },
            ui: UiConfig { address: ui_addr },
        })
    }

    pub fn validate(&self) -> anyhow::Result<()> {
        if self.proxy.old_servers.is_empty() {
            anyhow::bail!("At least one old server must be specified");
        }

        if self.proxy.new_servers.is_empty() {
            anyhow::bail!("At least one new server must be specified");
        }

        for server in &self.proxy.old_servers {
            if !server.starts_with("http://") && !server.starts_with("https://") {
                anyhow::bail!("Old server URL must start with http:// or https://: {}", server);
            }
        }

        for server in &self.proxy.new_servers {
            if !server.starts_with("http://") && !server.starts_with("https://") {
                anyhow::bail!("New server URL must start with http:// or https://: {}", server);
            }
        }

        Ok(())
    }
}

fn default_proxy_addr() -> SocketAddr {
    "0.0.0.0:8081".parse().unwrap()
}

fn default_ui_addr() -> SocketAddr {
    "0.0.0.0:9000".parse().unwrap()
}

fn default_timeout() -> u64 {
    15
}

fn default_max_concurrent() -> usize {
    1000
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_config_validation_requires_old_servers() {
        let config = Config {
            proxy: ProxyConfig {
                address: default_proxy_addr(),
                old_servers: vec![],
                new_servers: vec!["http://localhost:8082".to_string()],
                old_servers_timeout_secs: 15,
                new_servers_timeout_secs: 15,
                old_servers_additional_headers: vec![],
                new_servers_additional_headers: vec![],
                max_concurrent_requests: 1000,
            },
            ui: UiConfig {
                address: default_ui_addr(),
            },
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_config_validation_requires_new_servers() {
        let config = Config {
            proxy: ProxyConfig {
                address: default_proxy_addr(),
                old_servers: vec!["http://localhost:8080".to_string()],
                new_servers: vec![],
                old_servers_timeout_secs: 15,
                new_servers_timeout_secs: 15,
                old_servers_additional_headers: vec![],
                new_servers_additional_headers: vec![],
                max_concurrent_requests: 1000,
            },
            ui: UiConfig {
                address: default_ui_addr(),
            },
        };

        assert!(config.validate().is_err());
    }

    #[test]
    fn test_valid_config() {
        let config = Config {
            proxy: ProxyConfig {
                address: default_proxy_addr(),
                old_servers: vec!["http://localhost:8080".to_string()],
                new_servers: vec!["http://localhost:8082".to_string()],
                old_servers_timeout_secs: 15,
                new_servers_timeout_secs: 15,
                old_servers_additional_headers: vec![],
                new_servers_additional_headers: vec![],
                max_concurrent_requests: 1000,
            },
            ui: UiConfig {
                address: default_ui_addr(),
            },
        };

        assert!(config.validate().is_ok());
    }
}
