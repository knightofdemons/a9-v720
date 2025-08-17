use anyhow::Result;
use bytes::{Buf, BufMut, BytesMut};

/// Protocol header structure (20 bytes)
#[derive(Debug, Clone)]
pub struct ProtocolHeader {
    pub length: u32,       // Payload length (little-endian)
    pub cmd: u16,          // Command type (little-endian)
    pub msg_flag: u8,      // Message flag
    pub deal_fl: u8,       // Deal flag
    pub fwd_id: [u8; 8],   // Forward ID
    pub pkg_id: u32,       // Packet ID (little-endian)
}

impl ProtocolHeader {
    pub const SIZE: usize = 20;

    /// Parse header from bytes
    pub fn from_bytes(data: &[u8]) -> Result<(Self, &[u8])> {
        if data.len() < Self::SIZE {
            return Err(anyhow::anyhow!("Insufficient data for protocol header"));
        }

        // Debug: Print the first 20 bytes to understand the protocol format
        tracing::debug!("Raw protocol header bytes: {:?}", &data[..20.min(data.len())]);

        let mut buf = data;
        let length = buf.get_u32_le();
        let cmd = buf.get_u16_le();
        let msg_flag = buf.get_u8();
        let deal_fl = buf.get_u8();
        
        let mut fwd_id = [0u8; 8];
        buf.copy_to_slice(&mut fwd_id);
        
        let pkg_id = buf.get_u32_le();

        let header = Self {
            length,
            cmd,
            msg_flag,
            deal_fl,
            fwd_id,
            pkg_id,
        };

        tracing::debug!("Parsed header: length={}, cmd={}, msg_flag={}, deal_fl={}, pkg_id={}", 
            length, cmd, msg_flag, deal_fl, pkg_id);

        Ok((header, buf))
    }

    /// Serialize header to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(Self::SIZE);
        buf.put_u32_le(self.length);
        buf.put_u16_le(self.cmd);
        buf.put_u8(self.msg_flag);
        buf.put_u8(self.deal_fl);
        buf.put_slice(&self.fwd_id);
        buf.put_u32_le(self.pkg_id);
        buf.to_vec()
    }

    /// Create a new header
    pub fn new(cmd: u16, length: u32, msg_flag: u8, pkg_id: u32) -> Self {
        Self {
            length,
            cmd,
            msg_flag,
            deal_fl: 0,
            fwd_id: [0u8; 8],
            pkg_id,
        }
    }

    /// Create header for JSON message
    pub fn json(pkg_id: u32, json_length: usize) -> Self {
        Self::new(0, json_length as u32, 255, pkg_id)
    }

    /// Create header for binary message
    pub fn binary(cmd: u8, pkg_id: u32, data_length: usize) -> Self {
        Self::new(cmd.into(), data_length as u32, 255, pkg_id)
    }

    /// Create header for video frame
    pub fn video_frame(pkg_id: u32, frame_length: usize, msg_flag: u8) -> Self {
        Self::new(1, frame_length as u32, msg_flag, pkg_id)
    }

    /// Create header for audio frame
    pub fn audio_frame(pkg_id: u32, audio_length: usize) -> Self {
        Self::new(4, audio_length as u32, 255, pkg_id)
    }

    /// Create header for heartbeat
    pub fn heartbeat(pkg_id: u32) -> Self {
        Self::new(100, 20, 255, pkg_id) // 20-byte heartbeat
    }

    /// Create header for retransmission confirmation
    /// Note: Protocol spec says CMD=605, but u8 can only hold 0-255
    /// Using 245 as a placeholder until we verify the correct CMD value
    pub fn retransmission(pkg_id: u32, data_length: usize) -> Self {
        Self::new(245, data_length as u32, 255, pkg_id)
    }

    /// Create header for retransmission confirmation (code 605)
    /// Since u8 can't hold 605, we'll use a special value and handle it in the protocol layer
    pub fn retransmission_confirm(pkg_count: u32) -> Self {
        // Use CMD=245 as a placeholder for retransmission confirmation
        // The actual protocol expects CMD=605, but we'll handle this in the UDP layer
        Self::new(245, pkg_count * 4, 255, 0) // 4 bytes per package ID
    }
}

/// Retransmission confirmation structure
#[derive(Debug, Clone)]
pub struct RetransmissionConfirm {
    pub received_packets: Vec<u32>,
}

impl RetransmissionConfirm {
    /// Parse retransmission confirmation from bytes
    pub fn from_bytes(data: &[u8]) -> Result<Self> {
        if data.len() % 4 != 0 {
            return Err(anyhow::anyhow!("Invalid retransmission data length"));
        }

        let mut received_packets = Vec::new();
        let mut buf = data;

        while buf.remaining() >= 4 {
            received_packets.push(buf.get_u32_le());
        }

        Ok(Self { received_packets })
    }

    /// Serialize retransmission confirmation to bytes
    pub fn to_bytes(&self) -> Vec<u8> {
        let mut buf = BytesMut::with_capacity(self.received_packets.len() * 4);
        
        for &packet_id in &self.received_packets {
            buf.put_u32_le(packet_id);
        }

        buf.to_vec()
    }

    /// Create empty retransmission confirmation
    pub fn empty() -> Self {
        Self {
            received_packets: Vec::new(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_header_serialization() {
        let header = ProtocolHeader::json(123, 100);
        let bytes = header.to_bytes();
        let (parsed_header, _) = ProtocolHeader::from_bytes(&bytes).unwrap();
        
        assert_eq!(header.cmd, parsed_header.cmd);
        assert_eq!(header.length, parsed_header.length);
        assert_eq!(header.msg_flag, parsed_header.msg_flag);
        assert_eq!(header.pkg_id, parsed_header.pkg_id);
    }

    #[test]
    fn test_retransmission_serialization() {
        let confirm = RetransmissionConfirm {
            received_packets: vec![1, 2, 3, 4, 5],
        };
        let bytes = confirm.to_bytes();
        let parsed_confirm = RetransmissionConfirm::from_bytes(&bytes).unwrap();
        
        assert_eq!(confirm.received_packets, parsed_confirm.received_packets);
    }
}
