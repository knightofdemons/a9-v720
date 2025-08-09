use serde::{Deserialize, Serialize};
use anyhow::Result;
use tracing::{debug, error, info};

// Protocol constants (defined for reference but not currently used in code)
#[allow(dead_code)]
pub const CODE_C2S_REGISTER: i32 = 100;
#[allow(dead_code)]
pub const CODE_S2C_REGISTER_RSP: i32 = 101;
#[allow(dead_code)]
pub const CODE_S2D_NAT_REQ: i32 = 11;
#[allow(dead_code)]
pub const CODE_C2S_UDP_REQ: i32 = 20;
#[allow(dead_code)]
pub const CODE_S2C_UDP_RSP: i32 = 21;
#[allow(dead_code)]
pub const CODE_D2S_NAT_RSP: i32 = 12;
#[allow(dead_code)]
pub const CODE_C2D_PROBE_REQ: i32 = 50;
#[allow(dead_code)]
pub const CODE_D2C_PROBE_RSP: i32 = 51;
#[allow(dead_code)]
pub const CODE_S2_DEVICE_STATUS: i32 = 53;
#[allow(dead_code)]
pub const CODE_CMD_FORWARD: i32 = 301;
#[allow(dead_code)]
pub const CODE_RETRANSMISSION: i32 = 298;
#[allow(dead_code)]
pub const CODE_FORWARD_DEV_BASE_INFO: i32 = 4;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ProtocolMessage {
    pub code: i32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uid: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub status: Option<i32>,
    #[serde(rename = "cliTarget", skip_serializing_if = "Option::is_none")]
    pub cli_target: Option<String>,
    #[serde(rename = "cliToken", skip_serializing_if = "Option::is_none")]
    pub cli_token: Option<String>,
    #[serde(rename = "cliIp", skip_serializing_if = "Option::is_none")]
    pub cli_ip: Option<String>,
    #[serde(rename = "cliPort", skip_serializing_if = "Option::is_none")]
    pub cli_port: Option<i32>,
    #[serde(rename = "cliNatIp", skip_serializing_if = "Option::is_none")]
    pub cli_nat_ip: Option<String>,
    #[serde(rename = "cliNatPort", skip_serializing_if = "Option::is_none")]
    pub cli_nat_port: Option<i32>,
    #[serde(rename = "devIp", skip_serializing_if = "Option::is_none")]
    pub dev_ip: Option<String>,
    #[serde(rename = "devPort", skip_serializing_if = "Option::is_none")]
    pub dev_port: Option<i32>,
    #[serde(rename = "devNatIp", skip_serializing_if = "Option::is_none")]
    pub dev_nat_ip: Option<String>,
    #[serde(rename = "devNatPort", skip_serializing_if = "Option::is_none")]
    pub dev_nat_port: Option<i32>,
    #[serde(rename = "devTarget", skip_serializing_if = "Option::is_none")]
    pub dev_target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub ip: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub port: Option<i32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub content: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub unix_timer: Option<i64>,
}

pub fn parse_protocol_message(data: &[u8]) -> Result<ProtocolMessage> {
    if data.len() < 20 {
        return Err(anyhow::anyhow!("Message too short: {} bytes", data.len()));
    }

    // Parse header (first 18 bytes)
    let msg_len = u32::from_le_bytes([data[0], data[1], data[2], data[3]]);
    let msg_flag = data[4];
    let pkg_id = u32::from_le_bytes([data[5], data[6], data[7], data[8]]);
    let deal_fl = data[9];
    let fwd_id = &data[10..18];

    debug!("Protocol header: len={}, flag={}, pkg_id={}, deal_fl={}, fwd_id={:?}", 
           msg_len, msg_flag, pkg_id, deal_fl, fwd_id);

    // Extract JSON payload
    let json_start = 20; // Skip 18-byte header + 2 null bytes
    if data.len() <= json_start {
        return Err(anyhow::anyhow!("No JSON payload found"));
    }

    let json_data = &data[json_start..];
    let json_str = String::from_utf8_lossy(json_data);
    
    debug!("JSON payload: {}", json_str);

    // Parse JSON
    match serde_json::from_str::<ProtocolMessage>(&json_str) {
        Ok(message) => {
            debug!("Parsed protocol message: {:?}", message);
            Ok(message)
        }
        Err(e) => {
            error!("Failed to parse JSON: {}", e);
            error!("JSON string: {}", json_str);
            Err(anyhow::anyhow!("JSON parse error: {}", e))
        }
    }
}

// Removed - unused function

pub fn serialize_registration_response(message: &ProtocolMessage) -> Result<Vec<u8>> {
    // Match exact Python server format - 48 bytes total
    // JSON with spaces: {"code": 101, "status": 200} = 28 bytes
    let json_str = format!("{{\"code\": {}, \"status\": {}}}", 
                          message.code, 
                          message.status.unwrap_or(200));
    let json_bytes = json_str.as_bytes();
    
    info!("📤 Serializing registration response (Python format): {}", json_str);

    // Python server header format (16 bytes total):
    // Length (4 bytes): 1c00 0000 (28 in little endian)
    // Padding (4 bytes): 0000 0000  
    // Magic (8 bytes): 3030 3030 3030 3030 0000 0000
    let mut data = Vec::new();
    
    // Length (4 bytes) - JSON length only
    data.extend_from_slice(&(json_bytes.len() as u32).to_le_bytes());
    
    // Padding (4 bytes)
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    
    // Magic sequence (8 bytes) - exactly as seen in Python capture
    data.extend_from_slice(&[0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30]);
    data.extend_from_slice(&[0x00, 0x00, 0x00, 0x00]);
    
    // JSON payload
    data.extend_from_slice(json_bytes);

    info!("✅ Registration response serialized: {} bytes (Python format)", data.len());
    Ok(data)
}

// Removed - unused function

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_registration_message() {
        // Example registration message from fake_server.md
        let hex_data = "57000000000000000000000000000000000000007b22636f6465223a203130302c2022756964223a2022303830306330303132384638222c2022746f6b656e223a2022393165646634316622202c22646f6d61696e223a2022763732302e6e6178636c6f772e636f6d227d";
        
        let data = hex::decode(hex_data).unwrap();
        let message = parse_protocol_message(&data).unwrap();
        
        assert_eq!(message.code, 100);
        assert_eq!(message.uid, Some("0800c00128F8".to_string()));
        assert_eq!(message.token, Some("91edf41f".to_string()));
        assert_eq!(message.domain, Some("v720.naxclow.com".to_string()));
    }

    #[test]
    fn test_serialize_registration_response() {
        let message = ProtocolMessage {
            code: 101,
            status: Some(200),
            ..Default::default()
        };
        
        let data = serialize_protocol_message(&message).unwrap();
        let json_str = String::from_utf8_lossy(&data[18..]);
        
        assert!(json_str.contains("\"code\":101"));
        assert!(json_str.contains("\"status\":200"));
    }
}
