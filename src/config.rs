use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    // Network configuration for camera communication
    pub server_ip: String,
    pub http_port: u16,  // Port for camera HTTP requests (default: 80)
    pub web_port: u16,   // Port for camera web server (default: 8080)
    pub tcp_port: u16,   // Port for camera TCP connections (default: 6123)
    pub udp_port: u16,   // Port for camera UDP connections (default: 6123)
    
    // Domain configuration
    pub domain: String,

    // Protocol configuration
    pub is_bind: String,

    // Default values for responses
    pub default_status: u16,
    pub default_timeout: u16,
    
    // New architecture configuration
    pub ingress_capacity: usize,    // Bounded queue size for ingress frames
    pub max_inflight: usize,        // Maximum concurrent message processing
    pub worker_threads: usize,      // Number of worker threads
    pub max_frame_length: usize,    // Maximum frame size
    pub tcp_ports: Vec<u16>,        // Multiple TCP ports to listen on
    pub udp_ports: Vec<u16>,        // Multiple UDP ports to listen on
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            server_ip: "192.168.1.200".to_string(),
            http_port: 80,      // Camera HTTP requests
            web_port: 1234,    // Camera web server
            tcp_port: 6123,     // Camera TCP connections  
            udp_port: 6123,     // Camera UDP connections
            domain: "v720.naxclow.com".to_string(),
            is_bind: "1".to_string(),
            default_status: 200,
            default_timeout: 30,
            // New architecture defaults
            ingress_capacity: 8192,
            max_inflight: 256,
            worker_threads: 8,
            max_frame_length: 65536,
            tcp_ports: vec![6123, 53221, 41234],
            udp_ports: vec![6123, 53221, 41234],
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server_config: ServerConfig,
}

impl AppConfig {
    pub fn load(path: &str) -> Result<Self> {
        if Path::new(path).exists() {
            let content = fs::read_to_string(path)?;
            let config: AppConfig = serde_json::from_str(&content)?;
            Ok(config)
        } else {
            let config = Self::default();
            config.save(path)?;
            Ok(config)
        }
    }

    pub fn save(&self, path: &str) -> Result<()> {
        let content = serde_json::to_string_pretty(self)?;
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
            code: self.server_config.default_status as i32,
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
        Self {
            server_config: ServerConfig::default(),
        }
    }
}