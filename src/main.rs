use std::sync::Arc;
use tokio::sync::RwLock;
use tokio::net::TcpListener;
use tokio::net::UdpSocket;

mod config;
mod types;
mod protocol;
mod router;
mod web;

use crate::config::AppConfig;
use crate::router::{tcp::TcpRouter, udp::UdpRouter};
use crate::types::CameraManager;
use crate::web::server::start_web_server;

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    // Initialize logging
    tracing_subscriber::fmt::init();

    tracing::info!("Starting A9 V720 Camera Server");

    // Load configuration
    let config = AppConfig::load()?;
    tracing::info!(
        "Configuration loaded: server_ip={}, tcp_port={}, web_port={}",
        config.server_ip,
        config.tcp_protocol_port,
        config.web_port
    );

    // Create camera manager
    let camera_manager = Arc::new(RwLock::new(CameraManager::new(config.clone())));

    // Start TCP router
    let tcp_listener = TcpListener::bind(format!("0.0.0.0:{}", config.tcp_protocol_port)).await?;
    tracing::info!("TCP router listening on port {}", config.tcp_protocol_port);

    let tcp_router = TcpRouter::new(config.clone(), camera_manager.clone());
    let tcp_handle = tokio::spawn(async move {
        tcp_router.run(tcp_listener).await
    });

    // Start UDP routers on all three ports
    // UDP Protocol Port (6123)
    let udp_socket_1 = UdpSocket::bind(format!("0.0.0.0:{}", config.udp_protocol_port)).await?;
    let config_1 = config.clone();
    let camera_manager_1 = camera_manager.clone();
    let udp_handle_1 = tokio::spawn(async move {
        UdpRouter::start(udp_socket_1, camera_manager_1, config_1).await
    });
    
    // UDP Stream Port 1 (53221)
    let udp_socket_2 = UdpSocket::bind(format!("0.0.0.0:{}", config.udp_stream_port_1)).await?;
    let config_2 = config.clone();
    let camera_manager_2 = camera_manager.clone();
    let udp_handle_2 = tokio::spawn(async move {
        UdpRouter::start(udp_socket_2, camera_manager_2, config_2).await
    });
    
    // UDP Stream Port 2 (41234) - also used as "random" port for video streaming
    let udp_socket_3 = UdpSocket::bind(format!("0.0.0.0:{}", config.udp_stream_port_2)).await?;
    let config_3 = config.clone();
    let camera_manager_3 = camera_manager.clone();
    let udp_handle_3 = tokio::spawn(async move {
        UdpRouter::start(udp_socket_3, camera_manager_3, config_3).await
    });

    // Start HTTP registration server on port 80
    let registration_camera_manager = camera_manager.clone();
    let registration_handle = tokio::spawn(async move {
        start_web_server(registration_camera_manager, 80).await
    });

    // Start HTTP web interface on configured web port
    let web_camera_manager = camera_manager.clone();
    let web_port = config.web_port;
    let web_handle = tokio::spawn(async move {
        start_web_server(web_camera_manager, web_port).await
    });

    tracing::info!("Server started successfully. Waiting for connections...");

    // Wait for all components to complete
    tokio::select! {
        result = tcp_handle => {
            if let Err(e) = result {
                tracing::error!("TCP router failed: {}", e);
            }
        }
        result = udp_handle_1 => {
            if let Err(e) = result {
                tracing::error!("UDP router (Protocol Port) failed: {}", e);
            }
        }
        result = udp_handle_2 => {
            if let Err(e) = result {
                tracing::error!("UDP router (Stream Port 1) failed: {}", e);
            }
        }
        result = udp_handle_3 => {
            if let Err(e) = result {
                tracing::error!("UDP router (Stream Port 2) failed: {}", e);
            }
        }
        result = registration_handle => {
            if let Err(e) = result {
                tracing::error!("Registration server failed: {}", e);
            }
        }
        result = web_handle => {
            if let Err(e) = result {
                tracing::error!("Web interface failed: {}", e);
            }
        }
    }
    
    Ok(())
}
