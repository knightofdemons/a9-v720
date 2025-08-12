#!/usr/bin/env python3
"""
Packet capture script for A9 V720 camera communication analysis
Uses tcpdump to capture packets and analyze the protocol
"""

import subprocess
import sys
import time
import json
from datetime import datetime

def run_tcpdump(capture_time=60, interface="any"):
    """Run tcpdump to capture camera packets"""
    print(f"🔍 Starting packet capture for {capture_time} seconds on interface {interface}")
    print(f"📡 Capturing packets from/to 192.168.1.104 (camera IP)")
    
    # tcpdump command to capture camera traffic
    cmd = [
        "sudo", "tcpdump", "-v", "-i", interface,
        "-w", f"camera_capture_{datetime.now().strftime('%Y%m%d_%H%M%S')}.pcap",
        "host", "192.168.1.104", "and", "(port", "6123", "or", "port", "80)"
    ]
    
    print(f"🚀 Running: {' '.join(cmd)}")
    
    try:
        # Start tcpdump
        process = subprocess.Popen(cmd, stdout=subprocess.PIPE, stderr=subprocess.PIPE)
        
        print(f"⏱️  Capturing for {capture_time} seconds...")
        print("📋 Now trigger streaming or snapshot from the web interface")
        print("🔗 Visit: http://192.168.0.253:1234/")
        
        # Wait for capture time
        time.sleep(capture_time)
        
        # Stop tcpdump
        process.terminate()
        process.wait()
        
        print("✅ Packet capture completed")
        return True
        
    except KeyboardInterrupt:
        print("\n⏹️  Capture interrupted by user")
        process.terminate()
        return False
    except Exception as e:
        print(f"❌ Error during capture: {e}")
        return False

def analyze_pcap(pcap_file):
    """Analyze captured pcap file"""
    print(f"🔍 Analyzing {pcap_file}")
    
    # Use tcpdump to read and display the pcap file
    cmd = ["tcpdump", "-v", "-r", pcap_file]
    
    try:
        result = subprocess.run(cmd, capture_output=True, text=True)
        if result.returncode == 0:
            print("📊 Packet Analysis:")
            print("=" * 50)
            print(result.stdout)
        else:
            print(f"❌ Error reading pcap: {result.stderr}")
    except Exception as e:
        print(f"❌ Error analyzing pcap: {e}")

def main():
    print("🎥 A9 V720 Camera Packet Capture Tool")
    print("=" * 40)
    
    # Check if running as root
    if subprocess.run(["id", "-u"], capture_output=True, text=True).stdout.strip() != "0":
        print("⚠️  Warning: This script may need root privileges for tcpdump")
    
    # Capture packets
    if run_tcpdump(capture_time=30):
        # Find the most recent pcap file
        import glob
        pcap_files = glob.glob("camera_capture_*.pcap")
        if pcap_files:
            latest_pcap = max(pcap_files, key=lambda x: x.split('_')[1:])
            analyze_pcap(latest_pcap)
        else:
            print("❌ No pcap files found")
    else:
        print("❌ Packet capture failed")

if __name__ == "__main__":
    main()
