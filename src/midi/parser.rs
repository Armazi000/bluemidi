pub struct BleMidiParser {
    running_status: u8,
    sysex_buffer: Vec<u8>,
    in_sysex: bool,
    msg_buffer: [u8; 3],
    msg_len: usize,
    expected_data_len: usize,
}

impl BleMidiParser {
    pub fn new() -> Self {
        Self {
            running_status: 0,
            sysex_buffer: Vec::with_capacity(1024),
            in_sysex: false,
            msg_buffer: [0; 3],
            msg_len: 0,
            expected_data_len: 0,
        }
    }

    pub fn reset(&mut self) {
        self.running_status = 0;
        self.sysex_buffer.clear();
        self.in_sysex = false;
        self.msg_len = 0;
        self.expected_data_len = 0;
    }

    #[inline]
    pub fn parse<F>(&mut self, packet: &[u8], mut on_midi: F)
    where
        F: FnMut(&[u8]),
    {
        if packet.len() < 2 {
            return;
        }

        let header = packet[0];
        if (header & 0xC0) != 0x80 {
            return;
        }

        self.running_status = 0;
        self.msg_len = 0;
        self.expected_data_len = 0;

        let mut idx = 1;
        let len = packet.len();

        if self.in_sysex {
            while idx < len {
                let b = packet[idx];
                idx += 1;

                if b >= 0xF8 {
                    on_midi(&[b]);
                } else if b == 0xF7 {
                    self.sysex_buffer.push(0xF7);
                    self.in_sysex = false;
                    on_midi(&self.sysex_buffer);
                    self.sysex_buffer.clear();
                    break;
                } else if (b & 0x80) != 0 {
                    if b < 0xF8 {
                        self.sysex_buffer.clear();
                        self.in_sysex = false;
                        idx -= 1;
                        break;
                    }
                } else {
                    self.sysex_buffer.push(b);
                }
            }
        }

        while idx < len {
            let b = packet[idx];
            idx += 1;

            if b >= 0xF8 {
                on_midi(&[b]);
                continue;
            }

            if b == 0xF0 {
                self.in_sysex = true;
                self.sysex_buffer.clear();
                self.sysex_buffer.push(0xF0);
                self.running_status = 0;
                continue;
            } else if b == 0xF7 {
                if self.in_sysex {
                    self.sysex_buffer.push(0xF7);
                    self.in_sysex = false;
                    on_midi(&self.sysex_buffer);
                    self.sysex_buffer.clear();
                }
                self.running_status = 0;
                continue;
            } else if self.in_sysex {
                if (b & 0x80) == 0 {
                    self.sysex_buffer.push(b);
                } else {
                    if idx < len && packet[idx] == 0xF7 {
                        self.sysex_buffer.push(0xF7);
                        self.in_sysex = false;
                        on_midi(&self.sysex_buffer);
                        self.sysex_buffer.clear();
                        idx += 1;
                    } else if (b & 0x80) != 0 && b < 0xF8 {
                        self.sysex_buffer.clear();
                        self.in_sysex = false;
                        idx -= 1;
                    }
                }
                continue;
            }

            if (b & 0x80) != 0 {
                if idx < len {
                    let next = packet[idx];
                    if (next & 0x80) != 0 && next < 0xF8 {
                        idx += 1;
                        if next == 0xF0 {
                            self.in_sysex = true;
                            self.sysex_buffer.clear();
                            self.sysex_buffer.push(0xF0);
                            self.running_status = 0;
                        } else if next == 0xF7 {
                            if self.in_sysex {
                                self.sysex_buffer.push(0xF7);
                                self.in_sysex = false;
                                on_midi(&self.sysex_buffer);
                                self.sysex_buffer.clear();
                            }
                            self.running_status = 0;
                        } else {
                            self.set_status(next);
                        }
                        continue;
                    } else if (next & 0x80) == 0 && self.running_status != 0 {
                        continue;
                    }
                }

                self.set_status(b);
            } else {
                if self.running_status == 0 {
                    continue;
                }

                if self.msg_len == 0 {
                    self.msg_buffer[0] = self.running_status;
                    self.msg_buffer[1] = b;
                    self.msg_len = 2;
                } else if self.msg_len < 3 {
                    self.msg_buffer[self.msg_len] = b;
                    self.msg_len += 1;
                }

                if self.msg_len == 1 + self.expected_data_len {
                    on_midi(&self.msg_buffer[..self.msg_len]);
                    self.msg_len = 0;
                }
            }
        }
    }

    #[inline(always)]
    fn set_status(&mut self, status: u8) {
        if status < 0x80 || status >= 0xF8 {
            return;
        }

        let expected = match status & 0xF0 {
            0x80 => 2,
            0x90 => 2,
            0xA0 => 2,
            0xB0 => 2,
            0xC0 => 1,
            0xD0 => 1,
            0xE0 => 2,
            0xF0 => match status {
                0xF1 => 1,
                0xF2 => 2,
                0xF3 => 1,
                _ => 0,
            },
            _ => 0,
        };

        if status < 0xF0 {
            self.running_status = status;
        } else {
            self.running_status = 0;
        }

        self.expected_data_len = expected;
        self.msg_buffer[0] = status;
        self.msg_len = 1;

        if expected == 0 && status >= 0xF0 {
            self.msg_len = 1;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_single_note_on_off() {
        let mut parser = BleMidiParser::new();
        let mut received = Vec::new();

        let packet = [0x80, 0x80, 0x90, 60, 100];
        parser.parse(&packet, |msg| received.push(msg.to_vec()));

        assert_eq!(received.len(), 1);
        assert_eq!(received[0], vec![0x90, 60, 100]);

        received.clear();
        let packet_off = [0x80, 0x85, 0x80, 60, 0];
        parser.parse(&packet_off, |msg| received.push(msg.to_vec()));

        assert_eq!(received.len(), 1);
        assert_eq!(received[0], vec![0x80, 60, 0]);
    }

    #[test]
    fn test_mpe_expressions() {
        let mut parser = BleMidiParser::new();
        let mut received = Vec::new();

        let packet = [
            0x80,
            0x80, 0x91, 60, 127,
            0x81, 0xE1, 0x00, 0x40,
            0x82, 0xB1, 74, 95,
            0x83, 0xD1, 80,
            0x84, 0xA1, 60, 85,
        ];

        parser.parse(&packet, |msg| received.push(msg.to_vec()));

        assert_eq!(received.len(), 5);
        assert_eq!(received[0], vec![0x91, 60, 127]);
        assert_eq!(received[1], vec![0xE1, 0x00, 0x40]);
        assert_eq!(received[2], vec![0xB1, 74, 95]);
        assert_eq!(received[3], vec![0xD1, 80]);
        assert_eq!(received[4], vec![0xA1, 60, 85]);
    }

    #[test]
    fn test_running_status_multi_message() {
        let mut parser = BleMidiParser::new();
        let mut received = Vec::new();

        let packet = [
            0x80,
            0x80, 0xE2, 0x10, 0x40,
            0x81, 0x20, 0x40,
            0x82, 0x30, 0x40,
        ];

        parser.parse(&packet, |msg| received.push(msg.to_vec()));

        assert_eq!(received.len(), 3);
        assert_eq!(received[0], vec![0xE2, 0x10, 0x40]);
        assert_eq!(received[1], vec![0xE2, 0x20, 0x40]);
        assert_eq!(received[2], vec![0xE2, 0x30, 0x40]);
    }

    #[test]
    fn test_realtime_interleaving() {
        let mut parser = BleMidiParser::new();
        let mut received = Vec::new();

        let packet = [
            0x80,
            0x80, 0x90, 0xF8, 60, 100,
        ];

        parser.parse(&packet, |msg| received.push(msg.to_vec()));

        assert_eq!(received.len(), 2);
        assert_eq!(received[0], vec![0xF8]);
        assert_eq!(received[1], vec![0x90, 60, 100]);
    }

    #[test]
    fn test_multi_packet_sysex() {
        let mut parser = BleMidiParser::new();
        let mut received = Vec::new();

        let packet1 = [0x80, 0x80, 0xF0, 0x7E, 0x7F, 0x06, 0x01];
        parser.parse(&packet1, |msg| received.push(msg.to_vec()));
        assert_eq!(received.len(), 0);

        let packet2 = [0x80, 0x02, 0x03, 0xF7];
        parser.parse(&packet2, |msg| received.push(msg.to_vec()));

        assert_eq!(received.len(), 1);
        assert_eq!(received[0], vec![0xF0, 0x7E, 0x7F, 0x06, 0x01, 0x02, 0x03, 0xF7]);
    }
}
