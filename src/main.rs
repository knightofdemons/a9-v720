use axum::{
    extract::{Query, Request},
    response::Json,
    routing::post,
    Router,
    middleware::{self, Next},
    http::{Method, Uri, HeaderMap},
};
use serde::Deserialize;
use std::collections::HashMap;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::TcpListener;
use tokio::sync::RwLock;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, warn, error, debug};
use anyhow::Result;
use axum::response::Response;
use axum::body::Body;
use bytes::Bytes;


mod protocol;
mod camera_session;
mod config;
mod web;
use protocol::{ProtocolMessage, parse_protocol_message, serialize_registration_response, CODE_C2S_REGISTER};
use camera_session::CameraSession;
use config::AppConfig;

type CameraSessions = Arc<RwLock<HashMap<String, CameraSession>>>;

// Debug logging middleware to see raw requests and responses
async fn debug_logging_middleware(
    method: Method,
    uri: Uri,
    headers: HeaderMap,
    request: Request,
    next: Next,
) -> Response {
    info!("🔍 === RAW HTTP REQUEST ===");
    info!("Method: {}", method);
    info!("URI: {}", uri);
    info!("Headers: {:#?}", headers);
    
    // Extract request body for logging
    let (parts, body) = request.into_parts();
    let body_bytes = match axum::body::to_bytes(body, usize::MAX).await {
        Ok(bytes) => {
            if !bytes.is_empty() {
                info!("Request Body: {}", String::from_utf8_lossy(&bytes));
            } else {
                info!("Request Body: (empty)");
            }
            bytes
        }
        Err(e) => {
            error!("Failed to read request body: {}", e);
            Bytes::new()
        }
    };
    
    // Reconstruct request with the body we just read
    let request = Request::from_parts(parts, Body::from(body_bytes));
    
    // Process the request
    let response = next.run(request).await;
    
    // Log response details
    info!("🔍 === RAW HTTP RESPONSE ===");
    info!("Status: {}", response.status());
    info!("Response Headers: {:#?}", response.headers());
    
    // Note: We can't easily log response body without consuming it,
    // but the handlers will log their JSON responses
    
    response
}

// Using protocol constants from protocol.rs module instead

#[derive(Debug, Deserialize)]
struct RegisterDevicesQuery {
    _batch: String,
    _random: String,
    #[serde(rename = "devicesCode")]
    _devices_code: String,
}

#[derive(Debug, Deserialize)]
struct ConfirmDevicesQuery {
    _batch: String,
    _random: String,
    #[serde(rename = "devicesCode")]
    _devices_code: String,
}

#[derive(Debug, Deserialize)]
struct GetServerConfigQuery {
    #[serde(rename = "devicesCode")]
    devices_code: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env().add_directive(tracing::Level::INFO.into()))
        .init();

    info!("Starting A9 V720 Server...");

    let config = AppConfig::load("config.json")?;
    info!("Loaded configuration: {:?}", config);

    let camera_sessions: CameraSessions = Arc::new(RwLock::new(HashMap::new()));

    // Start HTTP server directly on port 80 (no proxy)
    let http_sessions = camera_sessions.clone();
    let http_config = config.clone();
    tokio::spawn(async move {
        start_http_server(http_sessions, http_config).await;
    });

    let tcp_sessions = camera_sessions.clone();
    let tcp_config = config.clone();
    tokio::spawn(async move {
        start_tcp_server(tcp_sessions, tcp_config).await;
    });

    // Start web management server
    let web_sessions = camera_sessions.clone();
    let web_config = config.clone();
    tokio::spawn(async move {
        info!("🌐 Starting web server task...");
        start_web_server(web_sessions, web_config).await;
        info!("🌐 Web server task completed");
    });

    // Start periodic session cleanup task
    let cleanup_sessions = camera_sessions.clone();
    tokio::spawn(async move {
        session_cleanup_task(cleanup_sessions).await;
    });

    // UDP servers removed - real protocol uses only HTTP + TCP with keepalives

    tokio::signal::ctrl_c().await?;
    info!("Shutting down server...");
    Ok(())
}

async fn start_http_server(_camera_sessions: CameraSessions, _config: AppConfig) {
    let app = Router::new()
        .route("/app/api/ApiSysDevicesBatch/registerDevices", post(register_devices))
        .route("/app/api/ApiSysDevicesBatch/confirm", post(confirm_devices))
        .route("/app/api/ApiServer/getA9ConfCheck", post(get_server_config))
        .fallback(handle_fallback)
        .layer(middleware::from_fn(debug_logging_middleware));

    let addr = "0.0.0.0:80"; // Bind directly to port 80
    info!("HTTP server listening on {}", addr);

    let listener = tokio::net::TcpListener::bind(addr).await.unwrap();
    axum::serve(listener, app).await.unwrap();
}

async fn handle_fallback(uri: axum::http::Uri) -> Json<serde_json::Value> {
    info!("INFO: [HTTP] Fallback handler for: {}", uri);
    
    let response = serde_json::json!({
        "code": 200,
        "message": "NAXCLOW API Gateway",
        "server": "v720.naxclow.com", 
        "status": "ok",
        "domain": "v720.naxclow.com"
    });

    info!("📤 Fallback Response: {}", serde_json::to_string(&response).unwrap_or_default());
    Json(response)
}

async fn register_devices(
    Query(query): Query<RegisterDevicesQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    info!("INFO: [HTTP] Device registration request");
    info!("📥 Register Query parameters: {:?}", query);

    let response = serde_json::json!({
        "code": 200,
        "message": "Registration successful",
        "data": {
            "serverIp": "192.168.1.200",
            "serverPort": 6123,
            "token": "camera_registration_token"
        }
    });

    info!("📤 Register Response: {}", serde_json::to_string(&response).unwrap_or_default());
    Ok(Json(response))
}

async fn confirm_devices(
    Query(query): Query<ConfirmDevicesQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    info!("INFO: [HTTP] Device confirmation request");
    info!("📥 Confirm Query parameters: {:?}", query);

    let response = serde_json::json!({
        "code": 200,
        "message": "Confirmation successful"
    });

    info!("📤 Confirm Response: {}", serde_json::to_string(&response).unwrap_or_default());
    Ok(Json(response))
}

async fn get_server_config(
    Query(query): Query<GetServerConfigQuery>,
) -> Result<Json<serde_json::Value>, (axum::http::StatusCode, String)> {
    info!("INFO: [HTTP] Getting server configuration");
    info!("📥 GetServerConfig Query parameters: {:?}", query);

    // Generate current Unix timestamp
    let current_time = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs()
        .to_string();

    let response = serde_json::json!({
        "code": 200,
        "message": "操作成功",
        "data": {
            "tcpPort": 6123,
            "uid": query.devices_code,
            "isBind": "1",
            "domain": "v720.naxclow.com", 
            "updateUrl": null,
            "host": "192.168.1.200",
            "currTime": current_time,
            "pwd": "a9camera2024",
            "version": null
        }
    });

    info!("📤 GetServerConfig Response: {}", serde_json::to_string(&response).unwrap_or_default());
    Ok(Json(response))
}

async fn start_tcp_server(camera_sessions: CameraSessions, config: AppConfig) {
    let addr = config.get_tcp_bind_addr();
    let listener = TcpListener::bind(addr.clone()).await.unwrap();
    info!("TCP server listening on {}", addr);

    loop {
        match listener.accept().await {
            Ok((socket, addr)) => {
                info!("TCP connection from {}", addr);
                let sessions = camera_sessions.clone();
                let tcp_config = config.clone();
                tokio::spawn(async move {
                    handle_tcp_connection(socket, addr, sessions, tcp_config).await;
                });
            }
            Err(e) => {
                error!("TCP accept error: {}", e);
            }
        }
    }
}

// UDP server function removed - not needed for real protocol

async fn handle_tcp_connection(
    mut socket: tokio::net::TcpStream,
    addr: SocketAddr,
    camera_sessions: CameraSessions,
    config: AppConfig,
) {
    let mut buffer = [0; 4096];
    
    loop {
        match socket.read(&mut buffer).await {
            Ok(0) => {
                info!("TCP connection closed from {}", addr);
                
                // Clean up the session for this disconnected camera
                {
                    let mut sessions = camera_sessions.write().await;
                    // Find and update session status for cameras from this IP
                    for (device_id, session) in sessions.iter_mut() {
                        if session.ip_address == addr.ip() {
                            session.protocol_state = crate::camera_session::ProtocolState::Disconnected;
                            info!("🔌 Marked camera {} ({}) as disconnected", device_id, addr.ip());
                            break;
                        }
                    }
                }
                break;
            }
            Ok(n) => {
                info!("INFO: [TCP] TCP Message Received from {}: {} bytes", addr, n);
                let data = &buffer[..n];
                
                // Handle different message types based on size and content
                if n == 20 {
                    // This is likely a keepalive message (20 bytes as seen in capture)
                    info!("🔄 Received keepalive from camera {}", addr);
                    
                    // Update the session's last_seen timestamp for any camera from this IP
                    {
                        let mut sessions = camera_sessions.write().await;
                        // Find session by IP address and update last_seen
                        for (device_id, session) in sessions.iter_mut() {
                            if session.ip_address == addr.ip() {
                                session.last_seen = chrono::Utc::now();
                                info!("📋 Updated last_seen for camera: {} ({})", device_id, addr.ip());
                                break;
                            }
                        }
                    }
                    
                    // Send keepalive response (20 bytes with pattern from working Python server)
                    let response = [
                        0x00, 0x00, 0x00, 0x00, 0x64, 0x00, 0x00, 0x00,
                        0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30, 0x30,
                        0x00, 0x00, 0x00, 0x00
                    ];
                    if let Err(e) = socket.write_all(&response).await {
                        error!("Failed to send keepalive response: {}", e);
                    } else {
                        info!("📤 Sent keepalive response to {}", addr);
                    }
                } else {
                    // Try to parse as protocol message (registration, etc.)
                    match parse_protocol_message(data) {
                        Ok(message) => {
                            info!("INFO: [TCP] Parsing TCP Message: code={}", message.code);
                            handle_tcp_message(message, &mut socket, addr, camera_sessions.clone(), config.clone()).await;
                        }
                        Err(e) => {
                            warn!("Failed to parse TCP message from {}: {} (might be keepalive or unknown format)", addr, e);
                            debug!("Raw data: {}", hex::encode(data));
                        }
                    }
                }
            }
            Err(e) => {
                error!("TCP read error from {}: {}", addr, e);
                break;
            }
        }
    }
}

async fn handle_tcp_message(
    message: ProtocolMessage,
    socket: &mut tokio::net::TcpStream,
    addr: SocketAddr,
    camera_sessions: CameraSessions,
    _config: AppConfig,
) {
    match message.code {
        cmd if cmd == CODE_C2S_REGISTER => {
            info!("=== CAMERA REGISTRATION ===");
            handle_camera_registration(message, socket, addr, camera_sessions).await;
        }
        _ => {
            info!("Unhandled TCP message code: {} - camera will maintain connection with keepalives", message.code);
        }
    }
}

async fn handle_camera_registration(
    message: ProtocolMessage,
    socket: &mut tokio::net::TcpStream,
    addr: SocketAddr,
    camera_sessions: CameraSessions,
) {
    info!("📥 Processing camera registration from: {}", addr);
    info!("📥 Registration message: {}", serde_json::to_string(&message).unwrap_or_default());
    
    // Use the device ID from the message as primary session key, fallback to IP if not provided
    let device_id = message.uid.clone().unwrap_or_else(|| format!("camera_{}", addr.ip()));
    let session_id = device_id.clone();
    
    info!("🏷️  Registering camera with ID: {} from IP: {}", session_id, addr.ip());
    
    {
        let mut sessions = camera_sessions.write().await;
        let mut session = CameraSession::new(device_id.clone(), addr.ip());
        session.protocol_state = crate::camera_session::ProtocolState::Registered;
        session.ip_address = addr.ip();
        sessions.insert(session_id.clone(), session);
        
        info!("📋 Total cameras registered: {}", sessions.len());
        info!("📋 Active cameras: {:?}", sessions.keys().collect::<Vec<_>>());
    } // Drop the lock here
    
    // Send registration response (code 101) - minimal response as per fake-server.md
    let response_msg = ProtocolMessage {
        code: 101,
        status: Some(200),
        ..Default::default()
    };
    
    let response_json = serde_json::json!({
        "code": 101,
        "status": 200
    });
    
    info!("📤 Sending registration response: {}", serde_json::to_string(&response_json).unwrap_or_default());
    
    // Send the response back to the camera using special registration headers
    match serialize_registration_response(&response_msg) {
        Ok(response_data) => {
            if let Err(e) = socket.write_all(&response_data).await {
                error!("Failed to send registration response: {}", e);
            } else {
                info!("✅ Registration response sent successfully");
                info!("🔄 Camera {} now in standby mode - keeping TCP connection alive", 
                      message.uid.as_deref().unwrap_or("unknown"));
            }
        }
        Err(e) => {
            error!("Failed to serialize registration response: {}", e);
        }
    }
    
    info!("✅ Camera registration completed for: {}", addr);
}

// All NAT/UDP/probe functions removed - real protocol uses simple TCP keepalive

async fn start_web_server(camera_sessions: CameraSessions, config: AppConfig) {
    info!("🌐 Creating web server state...");
    let web_state = web::WebServerState {
        camera_sessions,
        config: config.clone(),
    };
    
    info!("🌐 Creating web router...");
    let app = web::create_web_router(web_state);
    
    info!("🌐 Getting web bind address...");
    let addr = config.get_web_bind_addr();
    
    info!("🌐 Web management server starting on {}", addr);
    info!("📋 Access camera management at: http://{}", addr.replace("0.0.0.0", "localhost"));
    
    info!("🌐 Binding to address: {}", addr);
    match tokio::net::TcpListener::bind(&addr).await {
        Ok(listener) => {
            info!("🌐 Web server bound successfully, starting axum serve...");
            if let Err(e) = axum::serve(listener, app).await {
                error!("🌐 Web server failed: {}", e);
            }
        }
        Err(e) => {
            error!("🌐 Failed to bind web server to {}: {}", addr, e);
        }
    }
    info!("🌐 Web server function ending");
}

async fn session_cleanup_task(camera_sessions: CameraSessions) {
    let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(60)); // Run every minute
    
    loop {
        interval.tick().await;
        
        let mut sessions_to_remove = Vec::new();
        {
            let sessions = camera_sessions.read().await;
            for (device_id, session) in sessions.iter() {
                // Remove sessions that haven't been seen for more than 10 minutes
                if !session.is_active() {
                    sessions_to_remove.push(device_id.clone());
                }
            }
        }
        
        if !sessions_to_remove.is_empty() {
            let mut sessions = camera_sessions.write().await;
            for device_id in sessions_to_remove {
                if let Some(removed_session) = sessions.remove(&device_id) {
                    info!("🧹 Cleaned up inactive camera session: {} ({})", device_id, removed_session.ip_address);
                }
            }
            info!("📊 Active camera sessions: {}", sessions.len());
        }
    }
}
