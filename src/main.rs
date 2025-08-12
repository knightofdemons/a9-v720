use std::sync::Arc;
use tokio::sync::{mpsc, Semaphore};
use tracing::{info, error};
use axum::{
    routing::{get, post},
    http::StatusCode,
    Json, Router,
};
use serde_json::json;
use crate::config::AppConfig;
use crate::types::CameraSession;
use crate::net::tcp::TcpRouter;
use crate::net::udp::UdpSender;
use crate::pipeline::WorkerPool;

mod config;
mod types;
mod net;
mod pipeline;
mod telemetry;
mod protocol;
mod web;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    eprintln!("🚀 Starting A9 V720 Server...");
    
    // Initialize telemetry
    eprintln!("🔧 Initializing telemetry...");
    telemetry::init_telemetry();

    // Load configuration
    eprintln!("🔧 Loading configuration...");
    let config = AppConfig::load("config.json")?;
    eprintln!("⚙️ Configuration loaded successfully");
    info!("⚙️ Configuration loaded: {:?}", config.server_config);
    
    // Create bounded ingress queue
    eprintln!("🔧 Creating ingress queue...");
    let (ingress_tx, ingress_rx) = mpsc::channel::<crate::types::RawFrame>(8192);
    
    // Create concurrency limiter
    eprintln!("🔧 Creating concurrency limiter...");
    let concurrency = Arc::new(Semaphore::new(256));
    
    // Create network components
    eprintln!("🔧 Creating network components...");
    let tcp_router = Arc::new(TcpRouter::new());
    // Create UDP sender for responses
    eprintln!("🔧 Creating UDP sender...");
    let udp_sender = Arc::new(UdpSender::new(config.server_config.udp_ports.clone()).await?);
    
    // Create camera sessions storage
    eprintln!("🔧 Creating camera sessions storage...");
    let camera_sessions = Arc::new(tokio::sync::RwLock::new(
        std::collections::HashMap::<String, CameraSession>::new()
    ));
    
    // Start TCP listeners
    eprintln!("🔧 Starting TCP listeners...");
    let tcp_ports = vec![config.server_config.tcp_port];
    let tcp_ingress_tx = ingress_tx.clone();
    let tcp_router_clone = tcp_router.clone();
    let _tcp_handle = tokio::spawn(async move {
        eprintln!("🔧 TCP listener task started");
        info!("🔧 TCP listener task started");
        if let Err(e) = crate::net::tcp::run_tcp_listener(tcp_ports, tcp_ingress_tx, tcp_router_clone).await {
            error!("TCP listener failed: {}", e);
        }
    });
    
    // Start UDP sockets
    eprintln!("🔧 Starting UDP sockets...");
    let udp_ports = config.server_config.udp_ports.clone();
    let udp_ingress_tx = ingress_tx.clone();
    let _udp_handle = tokio::spawn(async move {
        eprintln!("🔧 UDP socket task started");
        info!("🔧 UDP socket task started");
        if let Err(e) = crate::net::udp::run_udp_socket(udp_ports, udp_ingress_tx).await {
            error!("UDP socket failed: {}", e);
        }
    });
    
    // Start worker pool
    eprintln!("🔧 Creating worker pool...");
    let worker_pool = WorkerPool::new(
        ingress_rx,
        concurrency,
        tcp_router.clone(),
        udp_sender.clone(),
        camera_sessions.clone(),
    );
    
    // Start HTTP server for camera registration and API endpoints
    eprintln!("🔧 Starting HTTP server...");
    let http_config = config.clone();
    let _http_handle = tokio::spawn(async move {
        eprintln!("🔧 HTTP server task started");
        info!("🔧 HTTP server task started");
        if let Err(e) = start_http_server(http_config).await {
            error!("HTTP server failed: {}", e);
        }
    });

    eprintln!("🔧 Starting web server...");
    let web_port = config.server_config.web_port;
    let web_camera_sessions = camera_sessions.clone();

    let _web_handle = tokio::spawn(async move {
        eprintln!("🔧 Web server task started");
        info!("🔧 Web server task started");
        if let Err(e) = crate::web::start_web_server(web_port, web_camera_sessions).await {
            error!("Web server failed: {}", e);
        }
    });
    
    // Start worker pool in a separate task
    eprintln!("🔧 Starting worker pool task...");
    let worker_handle = tokio::spawn(async move {
        eprintln!("🔧 Worker pool task started");
        info!("🔧 Worker pool task started");
        worker_pool.run().await;
    });
    
    eprintln!("🔧 All tasks started, waiting for worker pool...");
    info!("🔧 All tasks started, waiting for worker pool...");
    
    // Keep the server running by waiting for the worker pool to complete
    // The worker pool will only exit if the ingress channel is closed
    if let Err(e) = worker_handle.await {
        eprintln!("❌ Worker pool failed: {:?}", e);
        error!("❌ Worker pool failed: {:?}", e);
    }
    
    eprintln!("🛑 Server shutdown complete");
    info!("🛑 Server shutdown complete");
    Ok(())
}

/// Start HTTP server for camera registration and API endpoints
async fn start_http_server(config: AppConfig) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    info!("🔧 Starting HTTP server setup...");
    
    let app = Router::new()
        .route("/app/api/ApiSysDevicesBatch/registerDevices", post(handle_register_devices))
        .route("/app/api/ApiSysDevicesBatch/confirm", post(handle_confirm_devices))
        .route("/app/api/ApiServer/getA9ConfCheck", post(handle_get_config))
        .route("/register", post(handle_camera_registration))
        .route("/config", get(handle_config_request));
    
    info!("🔧 HTTP routes configured");

    let addr = format!("0.0.0.0:{}", config.server_config.http_port);
    info!("🔧 Attempting to bind HTTP server to {}", addr);
    
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    info!("🌐 HTTP server listening on {}", addr);
    
    info!("🔧 Starting axum server...");
    axum::serve(listener, app).await?;
    
    info!("🔧 HTTP server stopped");
    Ok(())
}

/// Handle device registration (bootstrap)
async fn handle_register_devices(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let batch_default = "A9_X4_V12".to_string();
    let random_default = "DEFGHI".to_string();
    let token_default = "547d4ef98b".to_string();
    
    let batch = params.get("batch").unwrap_or(&batch_default);
    let random = params.get("random").unwrap_or(&random_default);
    let _token = params.get("token").unwrap_or(&token_default);
    
    info!("📝 Register devices request: batch={}, random={}", batch, random);
    
    // Generate device code based on batch and random
    let device_code = format!("0800c001{}", &random[..4].to_uppercase());
    
    let response = json!({
        "code": 200,
        "message": "操作成功",
        "data": device_code
    });
    
    (StatusCode::OK, Json(response))
}

/// Handle device confirmation
async fn handle_confirm_devices(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let devices_code_default = "0800c00128F8".to_string();
    let random_default = "NOPQRS".to_string();
    let token_default = "025d085049".to_string();
    
    let devices_code = params.get("devicesCode").unwrap_or(&devices_code_default);
    let random = params.get("random").unwrap_or(&random_default);
    let _token = params.get("token").unwrap_or(&token_default);
    
    info!("✅ Confirm devices request: devices_code={}, random={}", devices_code, random);
    
    let response = json!({
        "code": 200,
        "message": "操作成功",
        "data": null
    });
    
    (StatusCode::OK, Json(response))
}

/// Handle get configuration
async fn handle_get_config(
    axum::extract::Query(params): axum::extract::Query<std::collections::HashMap<String, String>>,
) -> (StatusCode, Json<serde_json::Value>) {
    let devices_code_default = "0800c00128F8".to_string();
    let random_default = "FGHIJK".to_string();
    let token_default = "68778db973".to_string();
    
    let devices_code = params.get("devicesCode").unwrap_or(&devices_code_default);
    let random = params.get("random").unwrap_or(&random_default);
    let _token = params.get("token").unwrap_or(&token_default);
    
    info!("⚙️ Get config request: devices_code={}, random={}", devices_code, random);
    
    let response = json!({
        "code": 200,
        "message": "操作成功",
        "data": {
            "tcpPort": 6123,
            "uid": devices_code,
            "isBind": "8",
            "domain": "v720.naxclow.com", 
            "updateUrl": null,
            "host": "192.168.1.200",
            "currTime": "1676097689",
            "pwd": "91edf41f",
            "version": null
        }
    });
    
    (StatusCode::OK, Json(response))
}

/// Handle camera registration (legacy endpoint)
async fn handle_camera_registration(
    Json(payload): Json<serde_json::Value>,
) -> (StatusCode, Json<serde_json::Value>) {
    info!("📝 Camera registration request: {:?}", payload);
    
    // Extract device information
    let device_id = payload["device_id"].as_str().unwrap_or("unknown");
    let ip = payload["ip"].as_str().unwrap_or("0.0.0.0");
    let _port = payload["port"].as_u64().unwrap_or(6123) as u16;
    
    // Generate device ID based on IP (last two digits)
    let device_id = if let Some(ip_parts) = ip.split('.').last() {
        if let Ok(last_octet) = ip_parts.parse::<u16>() {
            format!("cam{:02}", last_octet % 100)
        } else {
            device_id.to_string()
        }
    } else {
        device_id.to_string()
    };
    
    let response = json!({
        "device_id": device_id,
        "status": "registered",
        "code": 200
    });
    
    (StatusCode::OK, Json(response))
}

/// Handle configuration request (legacy endpoint)
async fn handle_config_request() -> (StatusCode, Json<serde_json::Value>) {
    let config = AppConfig::load("config.json").unwrap_or_default();
    let response = config.get_server_config_response("", "", "");
    
    let json_response = serde_json::json!({
        "code": response.code,
        "server_ip": response.server_ip,
        "tcp_port": response.tcp_port,
        "udp_port": response.udp_port,
        "domain": response.domain,
        "is_bind": response.is_bind,
        "time_out": response.time_out,
    });
    
    (StatusCode::OK, Json(json_response))
}
