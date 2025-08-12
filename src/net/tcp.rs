use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::mpsc;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, warn, error, instrument, debug};
use crate::types::{RawFrame, ConnId};

/// Run TCP listener on specified ports
#[instrument(skip(ingress_tx, tcp_router))]
pub async fn run_tcp_listener(
    ports: Vec<u16>,
    ingress_tx: mpsc::Sender<RawFrame>,
    tcp_router: Arc<TcpRouter>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut listeners = Vec::new();
    
    // Bind to all specified ports
    for port in ports {
        let listener = TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
        info!("🔌 TCP listener bound on port {}", port);
        listeners.push((port, listener));
    }

    // Accept connections on all ports
    let mut accept_futures = Vec::new();
    for (port, listener) in listeners {
        let ingress_tx = ingress_tx.clone();
        let tcp_router = tcp_router.clone();
        let accept_future = accept_loop(port, listener, ingress_tx, tcp_router);
        accept_futures.push(accept_future);
    }

    // Wait for all accept loops
    for future in accept_futures {
        if let Err(e) = future.await {
            error!("TCP accept loop failed: {}", e);
        }
    }

    Ok(())
}

/// Accept loop for a single TCP port
async fn accept_loop(
    port: u16,
    listener: TcpListener,
    ingress_tx: mpsc::Sender<RawFrame>,
    tcp_router: Arc<TcpRouter>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let mut connection_id = 0u64;
    
    loop {
        match listener.accept().await {
            Ok((stream, peer_addr)) => {
                connection_id += 1;
                let conn_id = ConnId(connection_id);
                
                info!("🔗 TCP connection {} from {} on port {}", conn_id.0, peer_addr, port);
                debug!("📊 Camera connected - Connection ID: {}, IP: {}, Port: {}", conn_id.0, peer_addr.ip(), peer_addr.port());
                
                // Spawn connection handler
                let ingress_tx = ingress_tx.clone();
                let tcp_router = tcp_router.clone();
                tokio::spawn(async move {
                    if let Err(e) = handle_tcp_connection(conn_id, stream, peer_addr, ingress_tx, tcp_router).await {
                        error!("TCP connection {} failed: {}", conn_id.0, e);
                    }
                });
            }
            Err(e) => {
                error!("TCP accept error on port {}: {}", port, e);
                break;
            }
        }
    }
    
    Ok(())
}

/// Handle individual TCP connection using bounded queue system
async fn handle_tcp_connection(
    conn_id: ConnId,
    mut stream: TcpStream,
    peer_addr: SocketAddr,
    ingress_tx: mpsc::Sender<RawFrame>,
    tcp_router: Arc<TcpRouter>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Create response channel for this connection
    let (response_tx, mut response_rx) = mpsc::channel::<Vec<u8>>(100);
    
    // Register this connection with the TcpRouter FIRST
    info!("🔧 Registering TCP connection {} with TcpRouter", conn_id.0);
    tcp_router.register_connection(conn_id, response_tx).await;
    info!("✅ TCP connection {} registered successfully", conn_id.0);
    
    // Small delay to ensure registration is complete before any data processing
    tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;
    
    let tcp_router_clone = tcp_router.clone();
    
    // Use tokio::select! to handle both reading and writing concurrently
    let mut read_buffer = vec![0u8; 4096];
    
    loop {
        tokio::select! {
            // Handle reading from the stream
            read_result = stream.read(&mut read_buffer) => {
                match read_result {
                    Ok(0) => {
                        info!("🔌 TCP connection {} closed by peer", conn_id.0);
                        debug!("📊 Camera disconnected - Connection ID: {}, IP: {}", conn_id.0, peer_addr.ip());
                        break;
                    }
                    Ok(n) => {
                        let bytes = bytes::Bytes::copy_from_slice(&read_buffer[..n]);
                        
                        // Enhanced debug logging for received data
                        debug!("📥 Received {} bytes from camera {} ({}): {:?}", n, conn_id.0, peer_addr.ip(), bytes);
                        
                        // Try to parse as JSON for better logging
                        if bytes.len() > 20 {
                            let json_part = &bytes[20..];
                            if let Ok(json_value) = serde_json::from_slice::<serde_json::Value>(json_part) {
                                debug!("📋 Camera {} JSON data: {}", conn_id.0, serde_json::to_string_pretty(&json_value).unwrap_or_default());
                            }
                        } else if bytes.len() == 20 {
                            debug!("💓 Keepalive from camera {} ({}): {:?}", conn_id.0, peer_addr.ip(), bytes);
                        }
                        
                        let frame = RawFrame {
                            conn_id: Some(conn_id),
                            addr: Some(peer_addr),
                            bytes,
                        };
                        
                        // Send to ingress queue with backpressure
                        if let Err(e) = ingress_tx.send(frame).await {
                            error!("Failed to send TCP frame to ingress queue: {}", e);
                            break;
                        }
                    }
                    Err(e) => {
                        error!("TCP read error on connection {}: {}", conn_id.0, e);
                        debug!("📊 Camera connection error - Connection ID: {}, IP: {}, Error: {}", conn_id.0, peer_addr.ip(), e);
                        break;
                    }
                }
            }
            
            // Handle writing responses to the stream
            response = response_rx.recv() => {
                match response {
                    Some(response_data) => {
                        debug!("📤 Sending {} bytes to camera {} ({}:{}): {:?}", response_data.len(), conn_id.0, peer_addr.ip(), peer_addr.port(), response_data);
                        if let Err(e) = stream.write_all(&response_data).await {
                            error!("Failed to write response to TCP connection {}: {}", conn_id.0, e);
                            break;
                        }
                    }
                    None => {
                        // Response channel closed, exit the loop
                        debug!("📊 Response channel closed for camera {} ({})", conn_id.0, peer_addr.ip());
                        break;
                    }
                }
            }
        }
    }
    
    // Remove connection when task ends
    tcp_router_clone.remove_connection(&conn_id).await;
    debug!("📊 Camera cleanup complete - Connection ID: {}, IP: {}", conn_id.0, peer_addr.ip());
    
    Ok(())
}

/// TCP response router for sending responses back to connections
#[derive(Debug)]
pub struct TcpRouter {
    connections: Arc<tokio::sync::RwLock<std::collections::HashMap<ConnId, mpsc::Sender<Vec<u8>>>>>,
}

impl TcpRouter {
    pub fn new() -> Self {
        Self {
            connections: Arc::new(tokio::sync::RwLock::new(std::collections::HashMap::new())),
        }
    }
    
    /// Register a new connection
    pub async fn register_connection(&self, conn_id: ConnId, tx: mpsc::Sender<Vec<u8>>) {
        let mut connections = self.connections.write().await;
        connections.insert(conn_id, tx);
        info!("📝 Registered TCP connection {}", conn_id.0);
    }
    
    /// Remove a connection
    pub async fn remove_connection(&self, conn_id: &ConnId) {
        let mut connections = self.connections.write().await;
        connections.remove(conn_id);
        info!("🗑️ Removed TCP connection {}", conn_id.0);
    }
    
    /// Send response to a specific connection
    pub async fn send_to_conn(&self, conn_id: &ConnId, response: Vec<u8>) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let connections = self.connections.read().await;
        info!("🔍 Looking for TCP connection {} in {} registered connections", conn_id.0, connections.len());
        if let Some(tx) = connections.get(conn_id) {
            info!("📤 Sending {} bytes to TCP connection {}", response.len(), conn_id.0);
            debug!("📤 TCP response data: {:?}", response);
            if let Err(e) = tx.send(response).await {
                error!("Failed to send response to TCP connection {}: {}", conn_id.0, e);
                return Err(e.into());
            }
        } else {
            warn!("TCP connection {} not found for response", conn_id.0);
            // Log all available connection IDs for debugging
            let conn_ids: Vec<u64> = connections.keys().map(|id| id.0).collect();
            warn!("Available connections: {:?}", conn_ids);
        }
        Ok(())
    }
    

}
