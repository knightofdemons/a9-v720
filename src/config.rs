use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    // Network configuration
    pub server_ip: String,
    pub http_port: u16,
    pub tcp_port: u16,
    pub udp_port: u16,

    // Domain configuration
    pub domain: String,

    // Protocol configuration
    pub is_bind: String,

    // Default values for responses
    pub default_status: i32,
    pub default_timeout: u32,
}

impl Default for ServerConfig {
    fn default() -> Self {
        ServerConfig {
            server_ip: "192.168.1.200".to_string(),
            http_port: 80,
            tcp_port: 6123,
            udp_port: 6124,
            domain: "v720.naxclow.com".to_string(),
            is_bind: "1".to_string(),
            default_status: 200,
            default_timeout: 30,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfigResponse {
    pub code: i32,
    pub server_ip: String,
    pub tcp_port: i32,
    pub udp_port: i32,
    pub domain: String,
    pub is_bind: String,
    pub time_out: i32,
}

#[derive(Debug, Clone)]
pub struct AppConfig {
    pub server_config: ServerConfig,
}

impl AppConfig {
    pub fn load(path: &str) -> Result<Self> {
        if Path::new(path).exists() {
            let content = fs::read_to_string(path)?;
            let server_config: ServerConfig = serde_json::from_str(&content)?;
            Ok(AppConfig { server_config })
        } else {
            let config = Self::default();
            config.save(path)?;
            Ok(config)
        }
    }

    pub fn save(&self, path: &str) -> Result<()> {
        let content = serde_json::to_string_pretty(&self.server_config)?;
        fs::write(path, content)?;
        Ok(())
    }

    #[allow(dead_code)]
    pub fn get_http_bind_addr(&self) -> String {
        format!("0.0.0.0:{}", self.server_config.http_port)
    }

    #[allow(dead_code)]
    pub fn get_tcp_bind_addr(&self) -> String {
        format!("0.0.0.0:{}", self.server_config.tcp_port)
    }

    #[allow(dead_code)]
    pub fn get_udp_bind_addr(&self) -> String {
        format!("0.0.0.0:{}", self.server_config.udp_port)
    }

    #[allow(dead_code)]
    pub fn get_server_config_response(&self, _uid: &str, _password: &str, _current_time: &str) -> ServerConfigResponse {
        ServerConfigResponse {
            code: self.server_config.default_status,
            server_ip: self.server_config.server_ip.clone(),
            tcp_port: self.server_config.tcp_port as i32,
            udp_port: self.server_config.udp_port as i32,
            domain: self.server_config.domain.clone(),
            is_bind: self.server_config.is_bind.clone(),
            time_out: self.server_config.default_timeout as i32,
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        AppConfig {
            server_config: ServerConfig::default(),
        }
    }
}