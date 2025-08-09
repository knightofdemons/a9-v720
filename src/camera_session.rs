use std::net::IpAddr;
use chrono::{DateTime, Utc};

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub enum ProtocolState {
    Disconnected,
    Connected,
    Registered,
    NATRequested,
    NATCompleted,
    ProbeCompleted,
    CommandMode,
    Ready,
}

impl Default for ProtocolState {
    fn default() -> Self {
        ProtocolState::Disconnected
    }
}

#[derive(Debug, Clone)]
#[allow(dead_code)]
pub struct CameraSession {
    pub device_id: String,
    pub ip_address: IpAddr,
    pub protocol_state: ProtocolState,
    pub cli_target: String,
    pub cli_token: String,
    pub dev_ip: Option<String>,
    pub dev_port: Option<i32>,
    pub dev_nat_ip: Option<String>,
    pub dev_nat_port: Option<i32>,
    pub udp_port: Option<i32>,
    pub last_seen: DateTime<Utc>,
}

impl CameraSession {
    pub fn new(device_id: String, ip_address: IpAddr) -> Self {
        Self {
            device_id,
            ip_address,
            protocol_state: ProtocolState::Connected,
            cli_target: String::new(),
            cli_token: String::new(),
            dev_ip: None,
            dev_port: None,
            dev_nat_ip: None,
            dev_nat_port: None,
            udp_port: None,
            last_seen: Utc::now(),
        }
    }

    #[allow(dead_code)]
    pub fn update_last_seen(&mut self) {
        self.last_seen = Utc::now();
    }

    #[allow(dead_code)]
    pub fn is_active(&self) -> bool {
        let duration = Utc::now() - self.last_seen;
        duration.num_seconds() < 300 // 5 minutes timeout
    }

    pub fn is_connected(&self) -> bool {
        !matches!(self.protocol_state, ProtocolState::Disconnected) && self.is_active()
    }

    pub fn get_state_string(&self) -> &'static str {
        match self.protocol_state {
            ProtocolState::Disconnected => "Disconnected",
            ProtocolState::Connected => "Connected",
            ProtocolState::Registered => "Registered",
            ProtocolState::NATRequested => "NAT Requested",
            ProtocolState::NATCompleted => "NAT Completed",
            ProtocolState::ProbeCompleted => "Probe Completed",
            ProtocolState::CommandMode => "Command Mode",
            ProtocolState::Ready => "Ready",
        }
    }
}

impl std::fmt::Display for CameraSession {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "CameraSession {{ device_id: {}, ip: {}, state: {} }}",
            self.device_id,
            self.ip_address,
            self.get_state_string()
        )
    }
}
