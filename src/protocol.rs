use serde::{Deserialize, Serialize};
use anyhow::Result;

/// Protocol message structure
#[derive(Debug, Serialize, Deserialize)]
pub struct ProtocolMessage {
    pub code: i32,
    pub uid: Option<String>,
    pub token: Option<String>,
    pub domain: Option<String>,
    pub status: Option<i32>,
    pub dev_ip: Option<String>,
    pub dev_port: Option<i32>,
    pub dev_nat_ip: Option<String>,
    pub dev_nat_port: Option<i32>,
    pub cli_target: Option<String>,
    pub cli_token: Option<String>,
    pub cli_ip: Option<String>,
    pub cli_port: Option<i32>,
    pub cli_nat_ip: Option<String>,
    pub cli_nat_port: Option<i32>,
}

/// Parse a protocol message from bytes
pub fn parse_protocol_message(data: &[u8]) -> Result<ProtocolMessage> {
    if data.len() < 20 {
        return Err(anyhow::anyhow!("Data too short for protocol message"));
    }
    
    // Extract JSON part (skip binary header)
    let json_part = &data[20..];
    let message: ProtocolMessage = serde_json::from_slice(json_part)?;
    
    Ok(message)
}

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
        
        let data = serialize_registration_response(&message).unwrap();
        let json_str = String::from_utf8_lossy(&data[18..]);
        
        assert!(json_str.contains("\"code\":101"));
        assert!(json_str.contains("\"status\":200"));
    }
}
