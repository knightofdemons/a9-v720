use serde::{Deserialize, Serialize};

// HTTP Messages

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigCheckRequest {
    pub devices_code: String,
    pub random: String,
    pub token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigCheckResponse {
    pub code: u32,
    pub message: String,
    pub data: ConfigData,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ConfigData {
    #[serde(rename = "tcpPort")]
    pub tcp_port: u16,
    pub uid: String,
    #[serde(rename = "isBind")]
    pub is_bind: String,
    pub domain: String,
    #[serde(rename = "updateUrl")]
    pub update_url: Option<String>,
    pub host: String,
    #[serde(rename = "currTime")]
    pub curr_time: String,
    pub pwd: String,
    pub version: Option<String>,
}

// TCP Protocol Messages

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationRequest {
    pub code: u32,
    pub uid: String,
    pub token: String,
    pub domain: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegistrationResponse {
    pub code: u32,
    pub status: u32,
}

// Simple Protocol Messages (from working Python script analysis)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotRequest {
    pub code: u32,
    pub uid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SnapshotResponse {
    pub code: u32,
    pub status: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingRequest {
    pub code: u32,
    pub uid: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StreamingResponse {
    pub code: u32,
    pub status: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatProbeRequest {
    pub code: u32,
    #[serde(rename = "cliTarget")]
    pub cli_target: String,
    #[serde(rename = "cliToken")]
    pub cli_token: String,
    #[serde(rename = "cliIp")]
    pub cli_ip: String,
    #[serde(rename = "cliPort")]
    pub cli_port: u16,
    #[serde(rename = "cliNatIp")]
    pub cli_nat_ip: String,
    #[serde(rename = "cliNatPort")]
    pub cli_nat_port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NatProbeResponse {
    pub code: u32,
    pub status: u32,
    #[serde(rename = "devIp")]
    pub dev_ip: String,
    #[serde(rename = "devPort")]
    pub dev_port: u16,
    #[serde(rename = "devNatIp")]
    pub dev_nat_ip: String,
    #[serde(rename = "devNatPort")]
    pub dev_nat_port: u16,
    #[serde(rename = "cliTarget")]
    pub cli_target: String,
    #[serde(rename = "cliToken")]
    pub cli_token: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStatusRequest {
    pub code: u32,
    pub status: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForwardCommand {
    pub code: u32,
    pub target: String,
    pub content: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceInfoResponse {
    pub code: u32,
    pub target: String,
    pub content: crate::types::DeviceInfo,
}

// UDP Protocol Messages

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpProbeRequest {
    pub code: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UdpProbeResponse {
    pub code: u32,
    pub ip: String,
    pub port: u16,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Code51Response {
    pub code: u32,
    pub dev_target: String,
    pub status: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Code50Request {
    pub code: u32,
}

// Message creation helpers

impl ConfigCheckResponse {
    pub fn new(device_id: &str, config: &crate::config::AppConfig) -> Self {
        Self {
            code: 200,
            message: "操作成功".to_string(),
            data: ConfigData {
                tcp_port: config.tcp_protocol_port,
                uid: device_id.to_string(),
                is_bind: "8".to_string(),
                domain: config.domain.clone(),
                update_url: None,
                host: config.server_ip.clone(),
                curr_time: chrono::Utc::now().timestamp().to_string(),
                pwd: config.server_token.clone(),
                version: None,
            },
        }
    }
}

impl RegistrationResponse {
    pub fn new() -> Self {
        Self {
            code: 101,
            status: 200,
        }
    }
}

impl NatProbeRequest {
    pub fn new(config: &crate::config::AppConfig) -> Self {
        Self {
            code: 11,
            cli_target: config.client_target.clone(),
            cli_token: config.client_token.clone(),
            cli_ip: "255.255.255.255".to_string(),
            cli_port: 0,
            cli_nat_ip: config.server_ip.clone(),
            cli_nat_port: config.udp_protocol_port,
        }
    }
}

impl UdpProbeResponse {
    pub fn new(config: &crate::config::AppConfig, port: u16) -> Self {
        Self {
            code: 21,
            ip: config.server_ip.clone(),
            port,
        }
    }
}

impl Code50Request {
    pub fn new() -> Self {
        Self { code: 50 }
    }
}

impl DeviceStatusRequest {
    pub fn new() -> Self {
        Self {
            code: 53,
            status: 1,
        }
    }
}

impl ForwardCommand {
    pub fn retransmission_request(config: &crate::config::AppConfig) -> Self {
        let content = serde_json::json!({
            "code": 298
        });
        
        Self {
            code: 301,
            target: config.client_target.clone(),
            content,
        }
    }

    pub fn device_info_request(config: &crate::config::AppConfig) -> Self {
        let content = serde_json::json!({
            "unixTimer": chrono::Utc::now().timestamp(),
            "code": 4
        });
        
        Self {
            code: 301,
            target: config.client_target.clone(),
            content,
        }
    }

    pub fn start_streaming_request(config: &crate::config::AppConfig) -> Self {
        let content = serde_json::json!({
            "code": 3
        });
        
        Self {
            code: 301,
            target: config.client_target.clone(),
            content,
        }
    }
}
