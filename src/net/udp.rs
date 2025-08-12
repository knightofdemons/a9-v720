use std::net::SocketAddr;
use tokio::net::UdpSocket;
use tokio::sync::mpsc;
use tracing::{info, error, debug, warn};
use crate::types::RawFrame;
use std::collections::HashMap;
use std::sync::Arc;
use tokio::sync::RwLock;
use crate::types::CameraSession;

/// Run UDP socket on specified ports
pub async fn run_udp_socket(
    ports: Vec<u16>,
    ingress_tx: mpsc::Sender<RawFrame>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    for port in ports {
        let socket = UdpSocket::bind(format!("0.0.0.0:{}", port)).await?;
        info!("🔌 UDP socket bound on port {}", port);
        
        let ingress_tx_clone = ingress_tx.clone();
        tokio::spawn(async move {
            if let Err(e) = recv_loop(port, socket, ingress_tx_clone).await {
                error!("UDP receive loop failed on port {}: {}", port, e);
            }
        });
    }
    Ok(())
}

/// Receive loop for UDP socket
async fn recv_loop(
    port: u16,
    socket: UdpSocket,
    ingress_tx: mpsc::Sender<RawFrame>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut buffer = vec![0u8; 65536];
    loop {
        match socket.recv_from(&mut buffer).await {
            Ok((n, addr)) => {
                let bytes = bytes::Bytes::copy_from_slice(&buffer[..n]);
                let frame = RawFrame {
                    conn_id: None,
                    addr: Some(addr),
                    bytes,
                };
                if let Err(e) = ingress_tx.send(frame).await {
                    error!("Failed to send UDP frame to ingress queue: {}", e);
                    break;
                }
            }
            Err(e) => {
                error!("UDP receive error on port {}: {}", port, e);
                break;
            }
        }
    }
    Ok(())
}

/// UDP sender for sending responses
pub struct UdpSender {
    // Don't bind to specific ports, create sockets as needed
}

impl UdpSender {
    pub async fn new(_ports: Vec<u16>) -> Result<Self, Box<dyn std::error::Error + Send + Sync>> {
        // Don't bind to ports here - we'll create sockets as needed for sending
        info!("🔌 UDP sender initialized (will create sockets as needed)");
        Ok(Self {})
    }

    /// Send data to a specific address
    pub async fn send_to(&self, addr: SocketAddr, data: &[u8]) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        // Create a temporary socket for sending
        let socket = UdpSocket::bind("0.0.0.0:0").await?; // Bind to any available port
        
        match socket.send_to(data, addr).await {
            Ok(_) => {
                debug!("📤 Sent UDP response to {}", addr);
                Ok(())
            }
            Err(e) => {
                warn!("Failed to send UDP response to {}: {}", addr, e);
                Err(e.into())
            }
        }
    }
}

/// UDP streaming receiver for handling live video streams
pub struct UdpStreamingReceiver {
    camera_sessions: Arc<RwLock<HashMap<String, CameraSession>>>,
}

impl UdpStreamingReceiver {
    pub fn new(camera_sessions: Arc<RwLock<HashMap<String, CameraSession>>>) -> Self {
        Self {
            camera_sessions,
        }
    }
    
    /// Process streaming data from a camera
    pub async fn process_stream_data(&self, addr: SocketAddr, data: &[u8]) {
        debug!("📹 Processing stream data from IP {}: {} bytes", addr.ip(), data.len());
        
        // Check if this looks like video data (JPEG, etc.)
        if self.is_video_data(data) {
            let mut sessions = self.camera_sessions.write().await;
            
            // Find camera session by IP address
            let mut found_session = None;
            let mut found_device_id = None;
            
            for (device_id, session) in sessions.iter_mut() {
                if session.addr.ip() == addr.ip() {
                    found_session = Some(session);
                    found_device_id = Some(device_id.clone());
                    break;
                }
            }
            
            if let (Some(session), Some(device_id)) = (found_session, found_device_id) {
                if session.streaming {
                    session.add_stream_data(data);
                    debug!("📹 Added {} bytes to stream buffer for camera {}", data.len(), device_id);
                } else {
                    debug!("📹 Received video data but camera {} is not streaming", device_id);
                }
            } else {
                debug!("📹 No session found for IP {}", addr.ip());
            }
        } else {
            debug!("📹 Non-video data from IP {}: {} bytes", addr.ip(), data.len());
        }
    }
    
    /// Check if data looks like video content
    fn is_video_data(&self, data: &[u8]) -> bool {
        if data.len() < 4 {
            return false;
        }
        
        // Check for JPEG magic number
        if data.len() >= 2 && data[0] == 0xFF && data[1] == 0xD8 {
            return true;
        }
        
        // Check for other video formats (H.264, etc.)
        if data.len() >= 4 {
            // H.264 start code
            if data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x00 && data[3] == 0x01 {
                return true;
            }
            if data[0] == 0x00 && data[1] == 0x00 && data[2] == 0x01 {
                return true;
            }
        }
        
        // Large packets are likely video data
        data.len() > 1024
    }
    

}
