use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, UdpSocket};
use tokio::sync::RwLock;
use tokio::time::Duration;
use tracing::{debug, error, info, warn};
use anyhow::Result;

use crate::config::AppConfig;
use crate::types::{CameraConnection, Message, ProtocolState};

    pub struct CameraPool {
        pub config: AppConfig,
        pub connections: Arc<RwLock<HashMap<String, CameraConnection>>>,
        tcp_listener: Option<TcpListener>,
        udp_socket: Option<UdpSocket>,
    }

impl CameraPool {
    pub fn new(config: AppConfig) -> Self {
        Self {
            config,
            connections: Arc::new(RwLock::new(HashMap::new())),
            tcp_listener: None,
            udp_socket: None,
        }
    }

    pub async fn run(&self) -> Result<()> {
        info!("🎥 Starting camera pool...");

        // Start TCP listener
        let tcp_addr = format!("0.0.0.0:{}", self.config.tcp_port);
        self.tcp_listener = Some(TcpListener::bind(&tcp_addr).await?);
        info!("🔌 TCP listener bound on {}", tcp_addr);

        // Start UDP socket
        let udp_addr = format!("0.0.0.0:{}", self.config.udp_ports[0]);
        self.udp_socket = Some(UdpSocket::bind(&udp_addr).await?);
        info!("📡 UDP socket bound on {}", udp_addr);

        // Start health check task
        let connections = self.connections.clone();
        let health_interval = Duration::from_millis(self.config.health_check_interval_ms);
        tokio::spawn(async move {
            Self::run_health_checks(connections, health_interval).await;
        });

        // Main event loop
        self.main_loop().await?;

        Ok(())
    }

    async fn main_loop(&mut self) -> Result<()> {
        let _tcp_buffer = vec![0u8; 8192];
        let mut udp_buffer = vec![0u8; 65536];

        loop {
            tokio::select! {
                // Handle new TCP connections
                Ok((socket, addr)) = self.tcp_listener.as_mut().unwrap().accept() => {
                    info!("🔌 New TCP connection from {}", addr);
                    self.handle_new_tcp_connection(socket, addr).await;
                }

                // Handle UDP messages
                Ok((n, addr)) = self.udp_socket.as_mut().unwrap().recv_from(&mut udp_buffer) => {
                    let data = &udp_buffer[..n];
                    self.handle_udp_message(addr, data).await;
                }
            }
        }
    }

    async fn handle_new_tcp_connection(&self, socket: tokio::net::TcpStream, addr: SocketAddr) {
        let device_id = format!("cam{}", addr.ip().to_string().split('.').last().unwrap_or("0"));
        
        // Create new camera connection
        let mut conn = CameraConnection::new(device_id.clone(), addr.ip(), addr);
        conn.tcp_conn = Some(socket);

        // Store connection
        {
            let mut connections = self.connections.write().await;
            connections.insert(device_id.clone(), conn);
        }

        info!("📷 New camera connection: {} from {}", device_id, addr);

        // Handle TCP messages for this connection
        let connections = self.connections.clone();
        let config = self.config.clone();
        let socket_clone = socket.try_clone().await.unwrap();
        tokio::spawn(async move {
            Self::handle_tcp_connection(device_id, socket_clone, connections, config).await;
        });
    }

    async fn handle_tcp_connection(
        device_id: String,
        socket: tokio::net::TcpStream,
        connections: Arc<RwLock<HashMap<String, CameraConnection>>>,
        config: AppConfig,
    ) {
        let mut buffer = vec![0u8; 8192];

        loop {
            match socket.readable().await {
                Ok(_) => {
                    match socket.try_read(&mut buffer) {
                        Ok(n) => {
                            if n == 0 {
                                break; // Connection closed
                            }
                            
                            let data = &buffer[..n];
                            Self::process_tcp_message(&device_id, data, &connections, &config).await;
                        }
                        Err(_) => break,
                    }
                }
                Err(_) => break,
            }
        }

        // Remove connection when socket closes
        let mut connections = connections.write().await;
        connections.remove(&device_id);
        info!("🔌 Camera {} disconnected", device_id);
    }

    async fn handle_udp_message(&self, addr: SocketAddr, data: &[u8]) {
        // Find camera by IP
        let mut connections = self.connections.write().await;
        let camera = connections.values_mut().find(|conn| conn.ip == addr.ip());

        if let Some(conn) = camera {
            if Self::is_video_data(data) {
                // Handle video data
                conn.add_video_frame(data).await;
                debug!("📹 Video frame from {}: {} bytes", addr, data.len());
            } else {
                // Handle protocol message
                Self::process_udp_message(conn, data, addr).await;
            }
        } else {
            debug!("📡 UDP message from unknown IP {}: {} bytes", addr.ip(), data.len());
        }
    }

    async fn process_tcp_message(
        device_id: &str,
        data: &[u8],
        connections: &Arc<RwLock<HashMap<String, CameraConnection>>>,
        config: &AppConfig,
    ) {
        let mut connections = connections.write().await;
        if let Some(conn) = connections.get_mut(device_id) {
            match Self::parse_message(data) {
                Ok(Message::Keepalive) => {
                    conn.update_keepalive();
                    debug!("💓 Keepalive from camera {}", device_id);
                    
                    // Check for pending commands immediately
                    if let Some(command) = conn.take_pending_command() {
                        match command.as_str() {
                            "code11" => {
                                if let Err(e) = conn.send_code11_with_retry(config).await {
                                    error!("Failed to send Code 11 to camera {}: {}", device_id, e);
                                }
                            }
                            _ => {}
                        }
                    }
                }
                
                Ok(Message::Register { device_id, token, ip, port }) => {
                    info!("📝 Camera registration: {} from {}:{}", device_id, ip, port);
                    conn.token = token;
                }
                
                Ok(Message::NatProbeResponse { status, dev_ip, dev_port, dev_nat_ip: _, dev_nat_port, .. }) => {
                    info!("📡 NAT probe response from {}: status={}, dev_ip={}, dev_port={}", device_id, status, dev_ip, dev_port);
                    conn.camera_nat_port = Some(dev_nat_port);
                    conn.protocol_state = ProtocolState::WaitingForCode51;
                }
                
                _ => {
                    debug!("📨 Unknown TCP message from {}: {} bytes", device_id, data.len());
                }
            }
        }
    }

    async fn process_udp_message(conn: &mut CameraConnection, data: &[u8], addr: SocketAddr) {
        match Self::parse_message(data) {
            Ok(Message::UdpProbeRequest) => {
                info!("📡 UDP probe request from camera {}", conn.device_id);
                conn.send_code21_udp(addr).await;
                conn.protocol_state = ProtocolState::WaitingForCode51;
            }
            
            Ok(Message::ProbeResponse { device_target, status }) => {
                info!("📡 Probe response from camera {}: target={}, status={}", conn.device_id, device_target, status);
                conn.send_code50_udp(addr).await;
                conn.code51_count += 1;
                
                if conn.code51_count >= 2 {
                    conn.protocol_state = ProtocolState::Streaming;
                    info!("🎬 Streaming started for camera {}", conn.device_id);
                }
            }
            
            _ => {
                debug!("📨 Unknown UDP message from {}: {} bytes", conn.device_id, data.len());
            }
        }
    }

    async fn run_health_checks(
        connections: Arc<RwLock<HashMap<String, CameraConnection>>>,
        interval: Duration,
    ) {
        let mut ticker = tokio::time::interval(interval);
        
        loop {
            ticker.tick().await;
            
            let mut connections = connections.write().await;
            let mut to_remove = Vec::new();
            
            for (device_id, conn) in connections.iter_mut() {
                if !conn.is_connection_healthy() {
                    warn!("⚠️ Camera {} connection unhealthy, marking for removal", device_id);
                    to_remove.push(device_id.clone());
                }
            }
            
            for device_id in to_remove {
                connections.remove(&device_id);
                info!("🗑️ Removed unhealthy camera connection: {}", device_id);
            }
        }
    }

    fn is_video_data(data: &[u8]) -> bool {
        if data.len() < 4 {
            return false;
        }
        
        // Check for JPEG magic number
        if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
            return true;
        }
        
        // Check for H.264 start code
        if data.len() >= 4 && data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x00 && data[3] == 0x01 {
            return true;
        }
        
        // Large packets are likely video data
        data.len() > 1024
    }

    fn parse_message(data: &[u8]) -> Result<Message, Box<dyn std::error::Error + Send + Sync>> {
        // Check for 20-byte keepalive
        if data.len() == 20 {
            return Ok(Message::Keepalive);
        }
        
        // Try to parse JSON (skip binary header if present)
        let json_start = if data.len() > 20 { 20 } else { 0 };
        let json_part = &data[json_start..];
        
        if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(json_part) {
            if let Some(code) = json_value["code"].as_i64() {
                match code {
                    100 => {
                        let uid = json_value["uid"].as_str().unwrap_or("unknown").to_string();
                        let token = json_value["token"].as_str().map(|s| s.to_string());
                        let ip = json_value["ip"].as_str().unwrap_or("0.0.0.0").to_string();
                        let port = json_value["port"].as_u64().unwrap_or(6123) as u16;
                        
                        return Ok(Message::Register {
                            device_id: uid,
                            token,
                            ip,
                            port,
                        });
                    }
                    12 => {
                        let status = json_value["status"].as_u64().unwrap_or(0) as u16;
                        let dev_ip = json_value["devIp"].as_str().unwrap_or("").to_string();
                        let dev_port = json_value["devPort"].as_u64().unwrap_or(0) as u16;
                        let dev_nat_ip = json_value["devNatIp"].as_str().unwrap_or("").to_string();
                        let dev_nat_port = json_value["devNatPort"].as_u64().unwrap_or(0) as u16;
                        let cli_target = json_value["cliTarget"].as_str().unwrap_or("").to_string();
                        let cli_token = json_value["cliToken"].as_str().unwrap_or("").to_string();
                        
                        return Ok(Message::NatProbeResponse {
                            status,
                            dev_ip,
                            dev_port,
                            dev_nat_ip,
                            dev_nat_port,
                            cli_target,
                            cli_token,
                        });
                    }
                    20 => {
                        return Ok(Message::UdpProbeRequest);
                    }
                    51 => {
                        let device_target = json_value["devTarget"].as_str().unwrap_or("").to_string();
                        let status = json_value["status"].as_u64().unwrap_or(0) as u16;
                        
                        return Ok(Message::ProbeResponse {
                            device_target,
                            status,
                        });
                    }
                    _ => {
                        return Ok(Message::Unknown {
                            code: code as u16,
                            data: json_part.to_vec(),
                        });
                    }
                }
            }
        }
        
        Ok(Message::Unknown {
            code: 0,
            data: data.to_vec(),
        })
    }
}

// Extension methods for CameraConnection
impl CameraConnection {
    async fn send_code11_with_retry(&mut self, config: &AppConfig) -> Result<()> {
        let message = self.create_code11_message(config);
        self.send_with_retry(&message, "Code 11", config).await
    }

    async fn send_code21_udp(&mut self, addr: SocketAddr) {
        let message = self.create_code21_message();
        if let Err(e) = self.send_udp_message(addr, &message).await {
            warn!("Failed to send Code 21 via UDP: {}", e);
        } else {
            info!("📡 Code 21 sent to {} via UDP", addr);
        }
    }

    async fn send_code50_udp(&mut self, addr: SocketAddr) {
        let message = self.create_code50_message();
        if let Err(e) = self.send_udp_message(addr, &message).await {
            warn!("Failed to send Code 50 via UDP: {}", e);
        } else {
            info!("📡 Code 50 sent to {} via UDP", addr);
        }
    }

    async fn send_with_retry(&mut self, message: &[u8], message_type: &str, config: &AppConfig) -> Result<()> {
        let mut attempts = 0;
        
        while attempts < config.max_retries {
            match self.send_tcp_message(message).await {
                Ok(_) => {
                    info!("✅ {} sent successfully to camera {} (attempt {})", message_type, self.device_id, attempts + 1);
                    self.retry_count = 0;
                    return Ok(());
                }
                Err(e) => {
                    attempts += 1;
                    warn!("❌ Failed to send {} to camera {} (attempt {}): {}", message_type, self.device_id, attempts, e);
                    
                    if attempts < config.max_retries {
                        let delay = Duration::from_millis(100 * 2_u64.pow(attempts - 1));
                        tokio::time::sleep(delay).await;
                        
                        if let Err(_) = self.ensure_connection().await {
                            warn!("🔌 Reconnection failed for camera {}", self.device_id);
                        }
                    }
                }
            }
        }
        
        Err(anyhow::anyhow!("Failed to send {} after {} attempts", message_type, config.max_retries))
    }

    async fn ensure_connection(&mut self) -> Result<()> {
        if self.tcp_conn.is_none() || !self.is_connection_healthy() {
            info!("🔌 Reconnecting to camera {}", self.device_id);
            let addr = format!("{}:{}", self.ip, 6123);
            let socket = tokio::net::TcpStream::connect(&addr).await?;
            self.tcp_conn = Some(socket);
        }
        Ok(())
    }

    async fn send_tcp_message(&mut self, data: &[u8]) -> Result<()> {
        if let Some(ref mut conn) = self.tcp_conn {
            conn.writable().await?;
            conn.try_write(data)?;
            Ok(())
        } else {
            Err(anyhow::anyhow!("No TCP connection available"))
        }
    }

    async fn send_udp_message(&self, addr: SocketAddr, data: &[u8]) -> Result<()> {
        // This would need to be implemented with access to the UDP socket
        // For now, we'll use a simple approach
        let socket = UdpSocket::bind("0.0.0.0:0").await?;
        socket.send_to(data, addr).await?;
        Ok(())
    }

    fn create_code11_message(&self, config: &AppConfig) -> Vec<u8> {
        let code11_json = serde_json::json!({
            "code": 11,
            "cliTarget": "00112233445566778899aabbccddeeff",
            "cliToken": "deadc0de",
            "cliIp": "255.255.255.255",
            "cliPort": 0,
            "cliNatIp": config.server_ip,
            "cliNatPort": config.udp_ports[0]
        });
        
        let json_string = serde_json::to_string(&code11_json).unwrap();
        let json_bytes = json_string.as_bytes();
        let json_len = json_bytes.len() as u32;
        
        let mut payload = Vec::new();
        payload.extend_from_slice(&json_len.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.push(0);
        payload.push(0);
        payload.extend_from_slice(b"00000000");
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(json_bytes);
        
        payload
    }

    fn create_code21_message(&self) -> Vec<u8> {
        let code21_json = serde_json::json!({
            "code": 21,
            "ip": "192.168.1.99",
            "port": 32768 + (std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos() % 32767) as u16
        });
        
        let json_string = serde_json::to_string(&code21_json).unwrap();
        let json_bytes = json_string.as_bytes();
        let json_len = json_bytes.len() as u32;
        
        let mut payload = Vec::new();
        payload.extend_from_slice(&json_len.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.push(0);
        payload.push(0);
        payload.extend_from_slice(b"00000000");
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(json_bytes);
        
        payload
    }

    fn create_code50_message(&self) -> Vec<u8> {
        let code50_json = serde_json::json!({
            "code": 50
        });
        
        let json_string = serde_json::to_string(&code50_json).unwrap();
        let json_bytes = json_string.as_bytes();
        let json_len = json_bytes.len() as u32;
        
        let mut payload = Vec::new();
        payload.extend_from_slice(&json_len.to_le_bytes());
        payload.extend_from_slice(&0u16.to_le_bytes());
        payload.push(0);
        payload.push(0);
        payload.extend_from_slice(b"00000000");
        payload.extend_from_slice(&0u32.to_le_bytes());
        payload.extend_from_slice(json_bytes);
        
        payload
    }

    fn take_pending_command(&mut self) -> Option<String> {
        // For now, we'll implement this as a simple flag
        // In a real implementation, you'd have a proper pending command queue
        None
    }
}
