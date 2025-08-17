use serde::{Deserialize, Serialize};
use std::fs;
use anyhow::Result;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    pub server_ip: String,
    pub domain: String,
    pub client_target: String,
    pub client_token: String,
    pub server_token: String,
    
    pub tcp_registration_port: u16,
    pub tcp_protocol_port: u16,
    pub udp_protocol_port: u16,
    pub udp_stream_port_1: u16,
    pub udp_stream_port_2: u16,
    pub web_port: u16,
    
    pub max_retries: u32,
    pub retry_timeout_ms: u64,
    pub health_check_interval_ms: u64,
    pub retransmission_interval_ms: u64,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server_ip: "192.168.1.99".to_string(),
            domain: "v720.naxclow.com".to_string(),
            client_target: "00112233445566778899aabbccddeeff".to_string(),
            client_token: "deadc0de".to_string(),
            server_token: "deadbeef".to_string(),
            
            tcp_registration_port: 80,
            tcp_protocol_port: 6123,
            udp_protocol_port: 6123,
            udp_stream_port_1: 53221,
            udp_stream_port_2: 41234,
            web_port: 8080,
            
            max_retries: 3,
            retry_timeout_ms: 5000,
            health_check_interval_ms: 30000,
            retransmission_interval_ms: 100,
        }
    }
}

impl AppConfig {
    pub fn load() -> Result<Self> {
        // Try to load from config.json first
        if let Ok(config_str) = fs::read_to_string("config.json") {
            let config: AppConfig = serde_json::from_str(&config_str)?;
            return Ok(config);
        }
        
        // Fall back to default configuration
        tracing::warn!("config.json not found, using default configuration");
        Ok(AppConfig::default())
    }
    
    pub fn save(&self) -> Result<()> {
        let config_str = serde_json::to_string_pretty(self)?;
        fs::write("config.json", config_str)?;
        Ok(())
    }
}
