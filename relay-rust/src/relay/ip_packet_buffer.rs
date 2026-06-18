/*
 * Copyright (C) 2017 Genymobile
 *
 * Licensed under the Apache License, Version 2.0 (the "License");
 * you may not use this file except in compliance with the License.
 * You may obtain a copy of the License at
 *
 *     http://www.apache.org/licenses/LICENSE-2.0
 *
 * Unless required by applicable law or agreed to in writing, software
 * distributed under the License is distributed on an "AS IS" BASIS,
 * WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
 * See the License for the specific language governing permissions and
 * limitations under the License.
 */

use super::binary;
use super::byte_buffer::ByteBuffer;
use super::ip_packet::IpPacket;
use super::ipv4_packet::MAX_PACKET_LENGTH;

use byteorder::{BigEndian, ByteOrder};
use log::*;
use std::io;

pub struct IpPacketBuffer {
    buf: ByteBuffer,
}

impl IpPacketBuffer {
    pub fn new() -> Self {
        Self {
            buf: ByteBuffer::new(MAX_PACKET_LENGTH),
        }
    }

    pub fn read_from<R: io::Read>(&mut self, source: &mut R) -> io::Result<bool> {
        self.buf.read_from(source)
    }

    /// Peek at the packet version and total length from the buffered data.
    /// Returns `Some((version, total_length))` if enough data is available,
    /// or `None` if not enough data has been buffered yet.
    fn available_packet_info(&self) -> Option<(u8, u16)> {
        let data = self.buf.peek();
        trace!("Parse packet: {}", binary::build_packet_string(data));
        if data.len() < 4 {
            return None;
        }
        let version = data[0] >> 4;
        match version {
            4 => {
                // IPv4: total length at offset 2 (2 bytes)
                let length = BigEndian::read_u16(&data[2..4]);
                if length as usize <= data.len() {
                    Some((4, length))
                } else {
                    None
                }
            }
            6 => {
                // IPv6: payload_length at offset 4 (2 bytes); need at least 6 bytes
                if data.len() < 6 {
                    return None;
                }
                let payload_length = BigEndian::read_u16(&data[4..6]);
                let total_length = payload_length + 40; // 40-byte fixed header
                if total_length as usize <= data.len() {
                    Some((6, total_length))
                } else {
                    None
                }
            }
            _ => {
                warn!(target: "IpPacketBuffer", "Unknown IP version: {}", version);
                None
            }
        }
    }

    pub fn as_ip_packet(&mut self) -> Option<IpPacket<'_>> {
        if self.available_packet_info().is_some() {
            let data = self.buf.peek_mut();
            IpPacket::parse(data)
        } else {
            None
        }
    }

    pub fn next(&mut self) {
        // remove the packet in front of the buffer
        if let Some((_, length)) = self.available_packet_info() {
            self.buf.consume(length as usize);
        } // silently ignore if called without a packet
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::ipv4_header::Protocol;
    use crate::relay::transport_header::TransportHeaderData;
    use byteorder::{BigEndian, WriteBytesExt};
    use std::io;

    fn create_v4_packet() -> Vec<u8> {
        let mut raw = Vec::new();
        write_v4_packet_to(&mut raw);
        raw
    }

    fn write_v4_packet_to(raw: &mut Vec<u8>) {
        raw.write_u8(4u8 << 4 | 5).unwrap();
        raw.write_u8(0).unwrap(); // ToS
        raw.write_u16::<BigEndian>(32).unwrap(); // total length 20 + 8 + 4
        raw.write_u32::<BigEndian>(0).unwrap(); // id_flags_fragment_offset
        raw.write_u8(0).unwrap(); // TTL
        raw.write_u8(17).unwrap(); // protocol (UDP)
        raw.write_u16::<BigEndian>(0).unwrap(); // checksum
        raw.write_u32::<BigEndian>(0x12345678).unwrap(); // source address
        raw.write_u32::<BigEndian>(0x42424242).unwrap(); // destination address

        raw.write_u16::<BigEndian>(1234).unwrap(); // source port
        raw.write_u16::<BigEndian>(5678).unwrap(); // destination port
        raw.write_u16::<BigEndian>(12).unwrap(); // length
        raw.write_u16::<BigEndian>(0).unwrap(); // checksum

        raw.write_u32::<BigEndian>(0x11223344).unwrap(); // payload
    }

    fn write_v6_packet_to(raw: &mut Vec<u8>) {
        // IPv6 header (40 bytes)
        raw.write_u32::<BigEndian>(0x60000000).unwrap(); // version, traffic class, flow label
        raw.write_u16::<BigEndian>(12).unwrap(); // payload length (8 UDP + 4 payload)
        raw.write_u8(17).unwrap(); // next header (UDP)
        raw.write_u8(64).unwrap(); // hop limit
        // Source: 2001:db8::1
        raw.extend_from_slice(&[
            0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        ]);
        // Destination: 2001:db8::2
        raw.extend_from_slice(&[
            0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
        ]);

        // UDP header (8 bytes)
        raw.write_u16::<BigEndian>(1234).unwrap(); // source port
        raw.write_u16::<BigEndian>(5678).unwrap(); // destination port
        raw.write_u16::<BigEndian>(12).unwrap(); // length
        raw.write_u16::<BigEndian>(0).unwrap(); // checksum

        // Payload (4 bytes)
        raw.write_u32::<BigEndian>(0x11223344).unwrap();
    }

    fn check_v4_packet_headers(packet: &IpPacket) {
        let (ip_data, transport) = packet.headers_data();
        assert_eq!(Protocol::Udp, ip_data.protocol());
        if let Some(&TransportHeaderData::Udp(ref udp_header)) = transport {
            assert_eq!(1234, udp_header.source_port());
            assert_eq!(5678, udp_header.destination_port());
        } else {
            panic!("No UDP transport header");
        }
    }

    fn check_v6_packet_headers(packet: &IpPacket) {
        let (ip_data, transport) = packet.headers_data();
        assert_eq!(Protocol::Udp, ip_data.protocol());
        if let Some(&TransportHeaderData::Udp(ref udp_header)) = transport {
            assert_eq!(1234, udp_header.source_port());
            assert_eq!(5678, udp_header.destination_port());
        } else {
            panic!("No UDP transport header");
        }
    }

    #[test]
    fn parse_ipv4_packet_buffer() {
        let raw = create_v4_packet();
        let mut packet_buffer = IpPacketBuffer::new();

        let mut cursor = io::Cursor::new(raw);
        packet_buffer.read_from(&mut cursor).unwrap();

        let packet = packet_buffer.as_ip_packet().unwrap();
        check_v4_packet_headers(&packet);
    }

    #[test]
    fn parse_ipv6_packet_buffer() {
        let raw = {
            let mut raw = Vec::new();
            write_v6_packet_to(&mut raw);
            raw
        };
        let mut packet_buffer = IpPacketBuffer::new();

        let mut cursor = io::Cursor::new(raw);
        packet_buffer.read_from(&mut cursor).unwrap();

        let packet = packet_buffer.as_ip_packet().unwrap();
        check_v6_packet_headers(&packet);
    }

    #[test]
    fn parse_fragmented_ipv4_packet_buffer() {
        let raw = create_v4_packet();
        let mut packet_buffer = IpPacketBuffer::new();

        let mut cursor = io::Cursor::new(&raw[..14]);
        packet_buffer.read_from(&mut cursor).unwrap();

        assert!(packet_buffer.as_ip_packet().is_none());

        let mut cursor = io::Cursor::new(&raw[14..]);
        packet_buffer.read_from(&mut cursor).unwrap();

        let packet = packet_buffer.as_ip_packet().unwrap();
        check_v4_packet_headers(&packet);
    }

    fn create_multi_packets() -> Vec<u8> {
        let mut raw = Vec::new();
        write_v4_packet_to(&mut raw);
        write_v6_packet_to(&mut raw);
        write_v4_packet_to(&mut raw);
        raw
    }

    #[test]
    fn parse_multi_packets() {
        let raw = create_multi_packets();
        let mut packet_buffer = IpPacketBuffer::new();

        let mut cursor = io::Cursor::new(raw);
        packet_buffer.read_from(&mut cursor).unwrap();

        // First packet is v4
        check_v4_packet_headers(&packet_buffer.as_ip_packet().unwrap());
        packet_buffer.next();
        // Second packet is v6
        check_v6_packet_headers(&packet_buffer.as_ip_packet().unwrap());
        packet_buffer.next();
        // Third packet is v4
        check_v4_packet_headers(&packet_buffer.as_ip_packet().unwrap());
        packet_buffer.next();

        assert!(packet_buffer.as_ip_packet().is_none());
    }
}
