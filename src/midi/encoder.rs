pub struct BleMidiEncoder {
    max_packet_size: usize,
}

impl BleMidiEncoder {
    pub fn new(max_packet_size: usize) -> Self {
        Self {
            max_packet_size: max_packet_size.clamp(4, 512),
        }
    }

    pub fn encode(&self, midi: &[u8]) -> Vec<Vec<u8>> {
        if midi.is_empty() {
            return Vec::new();
        }

        if midi[0] != 0xF0 {
            let mut packet = Vec::with_capacity(2 + midi.len());
            packet.push(0x80);
            packet.push(0x80);
            packet.extend_from_slice(midi);
            return vec![packet];
        }

        let mut packets = Vec::new();
        let mut offset = 0;
        let total_len = midi.len();

        let first_chunk_capacity = self.max_packet_size.saturating_sub(2);
        let first_chunk_len = total_len.min(first_chunk_capacity);

        let mut first_packet = Vec::with_capacity(2 + first_chunk_len);
        first_packet.push(0x80);
        first_packet.push(0x80);
        first_packet.extend_from_slice(&midi[..first_chunk_len]);
        packets.push(first_packet);
        offset += first_chunk_len;

        while offset < total_len {
            let chunk_capacity = self.max_packet_size.saturating_sub(1);
            let chunk_len = (total_len - offset).min(chunk_capacity);

            let mut cont_packet = Vec::with_capacity(1 + chunk_len);
            cont_packet.push(0x80);
            cont_packet.extend_from_slice(&midi[offset..offset + chunk_len]);
            packets.push(cont_packet);
            offset += chunk_len;
        }

        packets
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_short_message() {
        let encoder = BleMidiEncoder::new(64);
        let note = [0x90, 60, 100];
        let packets = encoder.encode(&note);

        assert_eq!(packets.len(), 1);
        assert_eq!(packets[0], vec![0x80, 0x80, 0x90, 60, 100]);
    }

    #[test]
    fn test_encode_sysex_fragmentation() {
        let encoder = BleMidiEncoder::new(10);
        let sysex = vec![0xF0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0xF7];
        let packets = encoder.encode(&sysex);

        assert!(packets.len() > 1);
        assert_eq!(packets[0][0], 0x80);
        assert_eq!(packets[0][1], 0x80);
        assert_eq!(packets[0][2], 0xF0);
        assert_eq!(packets[1][0], 0x80);
        assert_ne!(packets[1][1], 0x80);
    }
}
