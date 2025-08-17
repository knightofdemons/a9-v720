use crate::types::CameraManager;
use axum::{
    extract::Query,
    http::StatusCode,
    response::{IntoResponse, Response},
    routing::{get, post},
    Json, Router,
};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tokio::sync::RwLock;
use tower_http::services::ServeDir;

use crate::web::camera_endpoints::*;

pub async fn start_web_server(
    camera_manager: Arc<RwLock<CameraManager>>,
    port: u16,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
    let app = Router::new()
        // Camera management endpoints
        .route("/api/cameras", get(list_cameras))
        .route("/api/cameras/:device_id", get(get_camera_info))
        .route("/api/cameras/:device_id/snapshot", post(trigger_snapshot))
        .route("/api/cameras/:device_id/stream", get(get_video_stream))
        .route("/api/cameras/:device_id/mjpeg", get(get_mjpeg_stream))
        .route("/api/cameras/:device_id/debug", get(debug_buffer))
        .route("/api/cameras/:device_id/streaming/start", get(start_streaming))
        .route("/api/cameras/:device_id/streaming/stop", get(stop_streaming))
        
        // Legacy endpoints for camera registration
        .route("/app/api/ApiServer/getA9ConfCheck", post(handle_config_check))
        .route("/app/api/ApiServer/getA9ConfCheck", get(handle_config_check))
        .route("/app/api/ApiSysDevicesBatch/registerDevices", post(handle_bootstrap_registration))
        .route("/app/api/ApiSysDevicesBatch/confirm", post(handle_bootstrap_confirm))
        
        // Web interface
        .route("/", get(serve_web_interface))
        .route("/dashboard", get(serve_dashboard))
        
        // Static files
        .nest_service("/static", ServeDir::new("static"))
        
        .with_state(camera_manager);

    let listener = tokio::net::TcpListener::bind(format!("0.0.0.0:{}", port)).await?;
    tracing::info!("HTTP server listening on port {}", port);

    axum::serve(listener, app).await?;
    Ok(())
}

async fn serve_web_interface() -> impl IntoResponse {
    let html = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>A9 V720 Camera Server</title>
    <style>
        body {
            font-family: Arial, sans-serif;
            margin: 0;
            padding: 20px;
            background-color: #2c2c2c;
            color: #ffffff;
        }
        .container {
            max-width: 1200px;
            margin: 0 auto;
            background: #3a3a3a;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.3);
        }
        h1 {
            color: #ffffff;
            text-align: center;
            margin-bottom: 30px;
        }
        .camera-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(400px, 1fr));
            gap: 20px;
            margin-top: 20px;
        }
        .camera-card {
            border: 1px solid #555;
            border-radius: 8px;
            padding: 15px;
            background: #4a4a4a;
        }
        .camera-header {
            display: flex;
            justify-content: space-between;
            align-items: center;
            margin-bottom: 15px;
        }
        .camera-ip {
            font-weight: bold;
            color: #ffffff;
        }
        .buffer-info {
            font-size: 12px;
            color: #cccccc;
            margin-top: 5px;
        }
        .camera-status {
            padding: 4px 8px;
            border-radius: 4px;
            font-size: 12px;
            font-weight: bold;
        }
        .status-connected {
            background: #d4edda;
            color: #155724;
        }
        .status-disconnected {
            background: #f8d7da;
            color: #721c24;
        }
        .video-container {
            position: relative;
            width: 100%;
            height: 300px;
            background: #000;
            border-radius: 4px;
            overflow: hidden;
        }
        .video-stream {
            width: 100%;
            height: 100%;
            object-fit: cover;
        }
        .camera-controls {
            margin-top: 10px;
            display: flex;
            gap: 10px;
        }
        .btn {
            padding: 8px 16px;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-size: 14px;
            text-decoration: none;
            display: inline-block;
            text-align: center;
        }
        .btn-primary {
            background: #007bff;
            color: white;
        }
        .btn-success {
            background: #28a745;
            color: white;
        }
        .btn:hover {
            opacity: 0.8;
        }
        .no-cameras {
            text-align: center;
            color: #cccccc;
            font-style: italic;
            padding: 40px;
        }
        .refresh-btn {
            background: #6c757d;
            color: white;
            padding: 10px 20px;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-size: 16px;
            margin-bottom: 20px;
        }
        .btn:disabled {
            opacity: 0.6;
            cursor: not-allowed;
        }
        @keyframes slideIn {
            from {
                transform: translateX(100%);
                opacity: 0;
            }
            to {
                transform: translateX(0);
                opacity: 1;
            }
        }
        @keyframes slideOut {
            from {
                transform: translateX(0);
                opacity: 1;
            }
            to {
                transform: translateX(100%);
                opacity: 0;
            }
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>🎥 A9 V720 Camera Server</h1>
        
        <button class="refresh-btn" onclick="loadCameras()">🔄 Refresh Cameras</button>
        
        <div id="camera-grid" class="camera-grid">
            <div class="no-cameras">Loading cameras...</div>
        </div>
    </div>

    <script>
        async function loadCameras() {
            try {
                const response = await fetch('/api/cameras');
                const data = await response.json();
                
                const grid = document.getElementById('camera-grid');
                
                if (data.data.cameras.length === 0) {
                    grid.innerHTML = '<div class="no-cameras">No cameras connected</div>';
                    return;
                }
                
                grid.innerHTML = '';
                
                for (const deviceId of data.data.cameras) {
                    const cameraCard = createCameraCard(deviceId);
                    grid.appendChild(cameraCard);
                    loadCameraInfo(deviceId, cameraCard);
                }
            } catch (error) {
                console.error('Error loading cameras:', error);
                document.getElementById('camera-grid').innerHTML = 
                    '<div class="no-cameras">Error loading cameras</div>';
            }
        }
        
        function createCameraCard(deviceId) {
            const card = document.createElement('div');
            card.className = 'camera-card';
            card.setAttribute('data-device-id', deviceId);
            card.innerHTML = `
                <div class="camera-header">
                    <span class="camera-device-id">${deviceId}</span>
                    <span class="camera-status status-disconnected">Disconnected</span>
                </div>
                <div class="camera-ip" id="ip-${deviceId}">IP: Loading...</div>
                <div class="buffer-info" id="buffer-${deviceId}">Buffer: 0 KB (0/100 frames)</div>
                <div class="video-container">
                    <img class="video-stream" id="video-${deviceId}" alt="Camera Stream" 
                         style="display:none;"
                         onerror="this.style.display='none'; this.nextElementSibling.style.display='block';">
                    <div id="no-video-${deviceId}" style="color:white; text-align:center; padding-top:130px;">
                        Click "Start Stream" to begin video streaming
                    </div>
                </div>
                <div class="camera-controls">
                    <button class="btn btn-primary" onclick="triggerSnapshot('${deviceId}')">📸 Snapshot</button>
                    <button class="btn btn-success" onclick="startStreaming('${deviceId}')" id="start-${deviceId}">▶️ Start Stream</button>
                    <button class="btn btn-danger" onclick="stopStreaming('${deviceId}')" id="stop-${deviceId}" style="display:none;">⏹️ Stop Stream</button>
                    <a class="btn btn-info" href="/api/cameras/${deviceId}/stream" target="_blank">🎬 View Stream</a>
                </div>
            `;
            return card;
        }
        
        async function loadCameraInfo(deviceId, card) {
            try {
                const response = await fetch(`/api/cameras/${deviceId}`);
                const data = await response.json();
                
                if (data.code === 200) {
                    const statusElement = card.querySelector('.camera-status');
                    const startBtn = card.querySelector(`#start-${deviceId}`);
                    const stopBtn = card.querySelector(`#stop-${deviceId}`);
                    const videoElement = card.querySelector(`#video-${deviceId}`);
                    const noVideoElement = card.querySelector(`#no-video-${deviceId}`);
                    const bufferElement = card.querySelector(`#buffer-${deviceId}`);
                    const ipElement = card.querySelector(`#ip-${deviceId}`);
                    
                    // Update IP address display
                    if (ipElement && data.data.ip) {
                        ipElement.textContent = `IP: ${data.data.ip}`;
                    }
                    
                    if (data.data.connected) {
                        statusElement.className = 'camera-status status-connected';
                        
                        // Update buffer information
                        if (bufferElement && data.data.stream_buffer) {
                            const frameCount = data.data.stream_buffer.frame_count || 0;
                            const maxFrames = data.data.stream_buffer.max_frames || 100;
                            const totalBytes = data.data.stream_buffer.total_bytes || 0;
                            const totalKB = Math.round(totalBytes / 1024);
                            bufferElement.textContent = `Buffer: ${totalKB} KB (${frameCount}/${maxFrames} frames)`;
                        }
                        
                        // Show streaming status and controls
                        if (data.data.streaming) {
                            statusElement.textContent = 'Streaming';
                            startBtn.style.display = 'none';
                            stopBtn.style.display = 'inline-block';
                            stopBtn.disabled = false;
                            stopBtn.textContent = '⏹️ Stop Stream';
                            
                            // Show video stream if camera is already streaming
                            if (videoElement && noVideoElement) {
                                // Check if we have video frames in buffer
                                if (data.data.stream_buffer && data.data.stream_buffer.frame_count > 0) {
                                    // Show buffer info instead of trying to display encoded video
                                    videoElement.style.display = 'none';
                                    noVideoElement.style.display = 'block';
                                    noVideoElement.innerHTML = `
                                        <div style="text-align: center; padding: 20px;">
                                            <h3>🎬 Video Streaming Active</h3>
                                            <p><strong>Buffer Status:</strong> ${data.data.stream_buffer.frame_count}/${data.data.stream_buffer.max_frames} frames</p>
                                            <p><strong>Total Data:</strong> ${Math.round(data.data.stream_buffer.total_bytes / 1024)} KB</p>
                                            <p><strong>Frame Size:</strong> ~${Math.round(data.data.stream_buffer.total_bytes / data.data.stream_buffer.frame_count)} bytes</p>
                                            <p style="color: #ffa500; margin-top: 15px;">
                                                <strong>Note:</strong> Video frames are encoded (H.264/H.265) and need decoding for display.<br>
                                                Raw video data is available for download.
                                            </p>
                                            <a href="/api/cameras/${deviceId}/stream" target="_blank" class="btn btn-info" style="margin-top: 10px;">
                                                📥 Download Latest Frame
                                            </a>
                                        </div>
                                    `;
                                } else {
                                    videoElement.style.display = 'none';
                                    noVideoElement.style.display = 'block';
                                    noVideoElement.textContent = 'Waiting for video frames...';
                                }
                                
                                // Set up periodic refresh for live video
                                if (window.videoRefreshIntervals && window.videoRefreshIntervals[deviceId]) {
                                    clearInterval(window.videoRefreshIntervals[deviceId]);
                                }
                                
                                if (!window.videoRefreshIntervals) {
                                    window.videoRefreshIntervals = {};
                                }
                                
                                window.videoRefreshIntervals[deviceId] = setInterval(() => {
                                    if (videoElement.style.display !== 'none') {
                                        videoElement.src = `/api/cameras/${deviceId}/stream?t=${Date.now()}`;
                                    }
                                }, 100); // Refresh every 100ms for 10 fps
                                
                                                            // Handle video load errors
                            videoElement.onerror = function() {
                                console.error('Video load error for camera:', deviceId);
                                this.style.display = 'none';
                                noVideoElement.style.display = 'block';
                                noVideoElement.textContent = 'Video stream not available...';
                                if (window.videoRefreshIntervals && window.videoRefreshIntervals[deviceId]) {
                                    clearInterval(window.videoRefreshIntervals[deviceId]);
                                }
                            };
                            
                            // Handle video load success
                            videoElement.onload = function() {
                                console.log('Video loaded successfully for camera:', deviceId);
                                noVideoElement.style.display = 'none';
                                this.style.display = 'block';
                            };
                            }
                        } else {
                            statusElement.textContent = 'Connected';
                            startBtn.style.display = 'inline-block';
                            stopBtn.style.display = 'none';
                            startBtn.disabled = false;
                            startBtn.textContent = '▶️ Start Stream';
                            
                            // Hide video stream if not streaming
                            if (videoElement && noVideoElement) {
                                videoElement.style.display = 'none';
                                videoElement.src = '';
                                noVideoElement.style.display = 'block';
                                noVideoElement.textContent = 'Click "Start Stream" to begin video streaming';
                            }
                        }
                    } else {
                        statusElement.textContent = 'Disconnected';
                        statusElement.className = 'camera-status status-disconnected';
                        startBtn.style.display = 'none';
                        stopBtn.style.display = 'none';
                        
                        // Reset buffer info
                        if (bufferElement) {
                            bufferElement.textContent = 'Buffer: 0 KB (0/100 frames)';
                        }
                        
                        // Hide video stream if disconnected
                        if (videoElement && noVideoElement) {
                            videoElement.style.display = 'none';
                            videoElement.src = '';
                            noVideoElement.style.display = 'block';
                            noVideoElement.textContent = 'Camera disconnected';
                        }
                    }
                }
            } catch (error) {
                console.error(`Error loading camera info for ${ip}:`, error);
            }
        }
        
        async function triggerSnapshot(deviceId) {
            try {
                const response = await fetch(`/api/cameras/${deviceId}/snapshot`, { method: 'POST' });
                const data = await response.json();
                
                if (data.code === 200) {
                    alert(`Snapshot triggered for ${deviceId}`);
                } else {
                    alert(`Error: ${data.message}`);
                }
            } catch (error) {
                console.error('Error triggering snapshot:', error);
                alert('Error triggering snapshot');
            }
        }
        
        async function startStreaming(deviceId) {
            try {
                const startBtn = document.querySelector(`#start-${deviceId}`);
                const stopBtn = document.querySelector(`#stop-${deviceId}`);
                const videoElement = document.querySelector(`#video-${deviceId}`);
                const noVideoElement = document.querySelector(`#no-video-${deviceId}`);
                
                // Disable button and show loading state
                startBtn.disabled = true;
                startBtn.textContent = '🔄 Starting...';
                
                const response = await fetch(`/api/cameras/${deviceId}/streaming/start`);
                const data = await response.json();
                
                if (data.code === 200) {
                    // Show success feedback
                    startBtn.style.display = 'none';
                    stopBtn.style.display = 'inline-block';
                    
                    // Update status
                    const statusElement = document.querySelector(`[data-device-id="${deviceId}"] .camera-status`);
                    if (statusElement) {
                        statusElement.textContent = 'Streaming';
                        statusElement.className = 'camera-status status-connected';
                    }
                    
                    // Start video stream after a short delay to allow camera to start streaming
                    setTimeout(() => {
                        if (videoElement && noVideoElement) {
                            // Use a simple approach: refresh the image every 100ms
                            videoElement.src = `/api/cameras/${deviceId}/stream?t=${Date.now()}`;
                            videoElement.style.display = 'block';
                            noVideoElement.style.display = 'none';
                            
                            // Set up periodic refresh for live video
                            if (window.videoRefreshIntervals && window.videoRefreshIntervals[deviceId]) {
                                clearInterval(window.videoRefreshIntervals[deviceId]);
                            }
                            
                            if (!window.videoRefreshIntervals) {
                                window.videoRefreshIntervals = {};
                            }
                            
                            window.videoRefreshIntervals[deviceId] = setInterval(() => {
                                if (videoElement.style.display !== 'none') {
                                    videoElement.src = `/api/cameras/${deviceId}/stream?t=${Date.now()}`;
                                }
                            }, 100); // Refresh every 100ms for 10 fps
                            
                            // Handle video load errors
                            videoElement.onerror = function() {
                                console.error('Video load error for camera:', deviceId);
                                this.style.display = 'none';
                                noVideoElement.style.display = 'block';
                                noVideoElement.textContent = 'Video stream not available yet...';
                                if (window.videoRefreshIntervals && window.videoRefreshIntervals[deviceId]) {
                                    clearInterval(window.videoRefreshIntervals[deviceId]);
                                }
                            };
                            
                            // Handle video load success
                            videoElement.onload = function() {
                                console.log('Video loaded successfully for camera:', deviceId);
                                noVideoElement.style.display = 'none';
                            };
                        }
                    }, 2000); // Wait 2 seconds for camera to start streaming
                    
                    // Show success message
                    showNotification(`Streaming started for ${deviceId}`, 'success');
                } else {
                    alert(`Error: ${data.message}`);
                    // Re-enable button
                    startBtn.disabled = false;
                    startBtn.textContent = '▶️ Start Stream';
                }
            } catch (error) {
                console.error('Error starting streaming:', error);
                alert('Error starting streaming');
                // Re-enable button
                const startBtn = document.querySelector(`#start-${deviceId}`);
                startBtn.disabled = false;
                startBtn.textContent = '▶️ Start Stream';
            }
        }
        
        async function stopStreaming(deviceId) {
            try {
                const startBtn = document.querySelector(`#start-${deviceId}`);
                const stopBtn = document.querySelector(`#stop-${deviceId}`);
                const videoElement = document.querySelector(`#video-${deviceId}`);
                const noVideoElement = document.querySelector(`#no-video-${deviceId}`);
                
                // Disable button and show loading state
                stopBtn.disabled = true;
                stopBtn.textContent = '🔄 Stopping...';
                
                const response = await fetch(`/api/cameras/${deviceId}/streaming/stop`);
                const data = await response.json();
                
                if (data.code === 200) {
                    // Show success feedback
                    stopBtn.style.display = 'none';
                    startBtn.style.display = 'inline-block';
                    startBtn.disabled = false;
                    startBtn.textContent = '▶️ Start Stream';
                    
                    // Update status
                    const statusElement = document.querySelector(`[data-device-id="${deviceId}"] .camera-status`);
                    if (statusElement) {
                        statusElement.textContent = 'Connected';
                        statusElement.className = 'camera-status status-connected';
                    }
                    
                    // Hide video stream and show placeholder
                    if (videoElement && noVideoElement) {
                        videoElement.style.display = 'none';
                        videoElement.src = '';
                        noVideoElement.style.display = 'block';
                        noVideoElement.textContent = 'Click "Start Stream" to begin video streaming';
                        
                        // Clear the refresh interval
                        if (window.videoRefreshIntervals && window.videoRefreshIntervals[deviceId]) {
                            clearInterval(window.videoRefreshIntervals[deviceId]);
                            delete window.videoRefreshIntervals[deviceId];
                        }
                    }
                    
                    // Show success message
                    showNotification(`Streaming stopped for ${deviceId}`, 'success');
                } else {
                    alert(`Error: ${data.message}`);
                    // Re-enable button
                    stopBtn.disabled = false;
                    stopBtn.textContent = '⏹️ Stop Stream';
                }
            } catch (error) {
                console.error('Error stopping streaming:', error);
                alert('Error stopping streaming');
                // Re-enable button
                const stopBtn = document.querySelector(`#stop-${deviceId}`);
                stopBtn.disabled = false;
                stopBtn.textContent = '⏹️ Stop Stream';
            }
        }
        
        function showNotification(message, type = 'info') {
            // Create notification element
            const notification = document.createElement('div');
            notification.className = `notification notification-${type}`;
            notification.textContent = message;
            notification.style.cssText = `
                position: fixed;
                top: 20px;
                right: 20px;
                padding: 12px 20px;
                border-radius: 4px;
                color: white;
                font-weight: bold;
                z-index: 1000;
                animation: slideIn 0.3s ease-out;
            `;
            
            // Set background color based on type
            if (type === 'success') {
                notification.style.backgroundColor = '#28a745';
            } else if (type === 'error') {
                notification.style.backgroundColor = '#dc3545';
            } else {
                notification.style.backgroundColor = '#007bff';
            }
            
            document.body.appendChild(notification);
            
            // Remove notification after 3 seconds
            setTimeout(() => {
                notification.style.animation = 'slideOut 0.3s ease-out';
                setTimeout(() => {
                    if (notification.parentNode) {
                        notification.parentNode.removeChild(notification);
                    }
                }, 300);
            }, 3000);
        }
        
        // Load cameras on page load
        loadCameras();
        
        // Refresh every 30 seconds
        setInterval(loadCameras, 30000);
    </script>
</body>
</html>
    "#;
    
    Response::builder()
        .status(200)
        .header("Content-Type", "text/html")
        .body(axum::body::Body::from(html))
        .unwrap()
}

async fn serve_dashboard() -> impl IntoResponse {
    let html = r#"
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>Camera Dashboard</title>
    <style>
        body {
            font-family: Arial, sans-serif;
            margin: 0;
            padding: 20px;
            background-color: #f5f5f5;
        }
        .container {
            max-width: 1400px;
            margin: 0 auto;
            background: white;
            padding: 20px;
            border-radius: 8px;
            box-shadow: 0 2px 10px rgba(0,0,0,0.1);
        }
        h1 {
            color: #333;
            text-align: center;
            margin-bottom: 30px;
        }
        .stats-grid {
            display: grid;
            grid-template-columns: repeat(auto-fit, minmax(200px, 1fr));
            gap: 20px;
            margin-bottom: 30px;
        }
        .stat-card {
            background: #f8f9fa;
            padding: 20px;
            border-radius: 8px;
            text-align: center;
            border-left: 4px solid #007bff;
        }
        .stat-number {
            font-size: 2em;
            font-weight: bold;
            color: #007bff;
        }
        .stat-label {
            color: #666;
            margin-top: 5px;
        }
        .camera-list {
            margin-top: 20px;
        }
        .camera-item {
            display: flex;
            justify-content: space-between;
            align-items: center;
            padding: 15px;
            border: 1px solid #ddd;
            border-radius: 8px;
            margin-bottom: 10px;
            background: #fafafa;
        }
        .camera-info {
            flex: 1;
        }
        .camera-ip {
            font-weight: bold;
            color: #333;
        }
        .camera-details {
            color: #666;
            font-size: 0.9em;
            margin-top: 5px;
        }
        .camera-actions {
            display: flex;
            gap: 10px;
        }
        .btn {
            padding: 8px 16px;
            border: none;
            border-radius: 4px;
            cursor: pointer;
            font-size: 14px;
            text-decoration: none;
            display: inline-block;
            text-align: center;
        }
        .btn-primary {
            background: #007bff;
            color: white;
        }
        .btn-success {
            background: #28a745;
            color: white;
        }
        .btn:hover {
            opacity: 0.8;
        }
    </style>
</head>
<body>
    <div class="container">
        <h1>📊 Camera Dashboard</h1>
        
        <div class="stats-grid">
            <div class="stat-card">
                <div class="stat-number" id="total-cameras">0</div>
                <div class="stat-label">Total Cameras</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" id="connected-cameras">0</div>
                <div class="stat-label">Connected</div>
            </div>
            <div class="stat-card">
                <div class="stat-number" id="active-streams">0</div>
                <div class="stat-label">Active Streams</div>
            </div>
        </div>
        
        <div class="camera-list" id="camera-list">
            <div style="text-align: center; color: #666; padding: 40px;">
                Loading cameras...
            </div>
        </div>
    </div>

    <script>
        async function loadDashboard() {
            try {
                const response = await fetch('/api/cameras');
                const data = await response.json();
                
                const cameras = data.data.cameras;
                document.getElementById('total-cameras').textContent = cameras.length;
                
                let connectedCount = 0;
                const cameraList = document.getElementById('camera-list');
                cameraList.innerHTML = '';
                
                for (const ip of cameras) {
                    const cameraInfo = await getCameraInfo(ip);
                    if (cameraInfo && cameraInfo.connected) {
                        connectedCount++;
                    }
                    
                    const cameraItem = createCameraItem(ip, cameraInfo);
                    cameraList.appendChild(cameraItem);
                }
                
                document.getElementById('connected-cameras').textContent = connectedCount;
                document.getElementById('active-streams').textContent = connectedCount;
                
            } catch (error) {
                console.error('Error loading dashboard:', error);
                document.getElementById('camera-list').innerHTML = 
                    '<div style="text-align: center; color: #666; padding: 40px;">Error loading cameras</div>';
            }
        }
        
        async function getCameraInfo(ip) {
            try {
                const response = await fetch(`/api/cameras/${ip}`);
                const data = await response.json();
                return data.code === 200 ? data.data : null;
            } catch (error) {
                console.error(`Error loading camera info for ${ip}:`, error);
                return null;
            }
        }
        
        function createCameraItem(ip, info) {
            const item = document.createElement('div');
            item.className = 'camera-item';
            
            const status = info && info.connected ? 'Connected' : 'Disconnected';
            const statusColor = info && info.connected ? '#28a745' : '#dc3545';
            
            item.innerHTML = `
                <div class="camera-info">
                    <div class="camera-ip">${ip}</div>
                    <div class="camera-details">
                        Status: <span style="color: ${statusColor};">${status}</span>
                        ${info ? `| Last heartbeat: ${new Date(info.last_heartbeat).toLocaleString()}` : ''}
                    </div>
                </div>
                <div class="camera-actions">
                    <button class="btn btn-primary" onclick="triggerSnapshot('${ip}')">📸 Snapshot</button>
                    <a class="btn btn-success" href="/api/cameras/${ip}/stream" target="_blank">🎬 Stream</a>
                </div>
            `;
            return item;
        }
        
        async function triggerSnapshot(ip) {
            try {
                const response = await fetch(`/api/cameras/${ip}/snapshot`, { method: 'POST' });
                const data = await response.json();
                
                if (data.code === 200) {
                    alert(`Snapshot triggered for ${ip}`);
                } else {
                    alert(`Error: ${data.message}`);
                }
            } catch (error) {
                console.error('Error triggering snapshot:', error);
                alert('Error triggering snapshot');
            }
        }
        
        // Load dashboard on page load
        loadDashboard();
        
        // Refresh every 30 seconds
        setInterval(loadDashboard, 30000);
    </script>
</body>
</html>
    "#;
    
    Response::builder()
        .status(200)
        .header("Content-Type", "text/html")
        .body(axum::body::Body::from(html))
        .unwrap()
}

#[derive(Debug, Deserialize)]
struct ConfigCheckParams {
    devicesCode: String,
    random: String,
    token: String,
}

async fn handle_config_check(
    Query(params): Query<ConfigCheckParams>,
) -> impl IntoResponse {
    tracing::info!(
        "Config check request (POST): {{\"devicesCode\": \"{}\", \"random\": \"{}\", \"token\": \"{}\"}}",
        params.devicesCode, params.random, params.token
    );

    let response = json!({
        "code": 200,
        "message": "OK",
        "data": {
            "uid": params.devicesCode,
            "host": "192.168.1.99",
            "domain": "v720.naxclow.com",
            "tcpPort": 6123,
            "pwd": "deadbeef",
            "isBind": "8",
            "currTime": chrono::Utc::now().timestamp().to_string(),
            "updateUrl": null,
            "version": null
        }
    });

    tracing::info!(
        "Config check response for device {}: {}",
        params.devicesCode,
        serde_json::to_string(&response).unwrap()
    );

    Json(response)
}

async fn handle_bootstrap_registration(
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    tracing::info!("Bootstrap registration request: {:?}", params);
    
    // Extract batch and random parameters
    let batch = params.get("batch").cloned().unwrap_or_else(|| "A9_48PIN_B".to_string());
    let random = params.get("random").cloned().unwrap_or_else(|| "DEFGHI".to_string());
    
    // Generate device ID from random parameter (like the archived version)
    let device_id = format!("0800c001{}", &random[..4].to_uppercase());
    
    // Create bootstrap response
    let response = json!({
        "code": 200,
        "message": "操作成功",
        "data": device_id
    });

    tracing::info!("Bootstrap registration response: {:?}", response);
    
    // Return custom response with specific headers to match pcap
    Response::builder()
        .status(StatusCode::OK)
        .header("Server", "nginx/1.14.0 (Ubuntu)")
        .header("Connection", "keep-alive")
        .header("Content-Type", "application/json; charset=utf-8")
        .header("Vary", "Origin")
        .header("Vary", "Access-Control-Request-Method")
        .header("Vary", "Access-Control-Request-Headers")
        .body(axum::body::Body::from(serde_json::to_string(&response).unwrap()))
        .unwrap()
}

async fn handle_bootstrap_confirm(
    Query(params): Query<std::collections::HashMap<String, String>>,
) -> impl IntoResponse {
    tracing::info!("Bootstrap confirmation request: {:?}", params);
    
    // Extract device ID from query parameters
    let device_id = params.get("devicesCode").cloned().unwrap_or_else(|| "unknown".to_string());
    
    // Create confirmation response
    let response = json!({
        "code": 200,
        "message": "操作成功",
        "data": null
    });

    tracing::info!("Bootstrap confirmation response for device {}: {:?}", device_id, response);
    
    // Return custom response with specific headers to match pcap
    Response::builder()
        .status(StatusCode::OK)
        .header("Server", "nginx/1.14.0 (Ubuntu)")
        .header("Connection", "keep-alive")
        .header("Content-Type", "application/json; charset=utf-8")
        .header("Vary", "Origin")
        .header("Vary", "Access-Control-Request-Method")
        .header("Vary", "Access-Control-Request-Headers")
        .body(axum::body::Body::from(serde_json::to_string(&response).unwrap()))
        .unwrap()
}
