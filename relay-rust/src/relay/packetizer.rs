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

use std::io;

use super::datagram::{DatagramReceiver, ReadAdapter};
use super::ip_header::{IpHeader, IpHeaderData, IpHeaderMut};
use super::ip_packet::{IpPacket, Ipv6Packet};
use super::ipv4_packet::{Ipv4Packet, MAX_PACKET_LENGTH};
use super::transport_header::{TransportHeader, TransportHeaderData, TransportHeaderMut};

/// Convert from level 5 to level 3 by appending correct IP and transport headers.
pub struct Packetizer {
    buffer: Box<[u8; MAX_PACKET_LENGTH]>,
    transport_index: usize,
    payload_index: usize,
    ip_header_data: IpHeaderData,
    transport_header_data: TransportHeaderData,
}

impl Packetizer {
    pub fn new(
        reference_ip_header: &IpHeader,
        reference_transport_header: &TransportHeader,
    ) -> Self {
        let mut buffer = Box::new([0; MAX_PACKET_LENGTH]);

        let transport_index = reference_ip_header.header_length() as usize;
        let payload_index = transport_index + reference_transport_header.header_length() as usize;

        let mut ip_header_data = reference_ip_header.data_clone();
        let mut transport_header_data = reference_transport_header.data_clone();

        {
            let ip_header_raw = &mut buffer[..transport_index];
            ip_header_raw.copy_from_slice(reference_ip_header.raw());
            let _ip_header = match &mut ip_header_data {
                IpHeaderData::V4(v4) => {
                    let mut h = v4.bind_mut(ip_header_raw);
                    h.swap_source_and_destination();
                    IpHeaderMut::V4(h)
                }
                IpHeaderData::V6(v6) => {
                    let mut h = v6.bind_mut(ip_header_raw);
                    h.swap_source_and_destination();
                    IpHeaderMut::V6(h)
                }
            };
        }

        {
            let transport_header_raw = &mut buffer[transport_index..payload_index];
            transport_header_raw.copy_from_slice(reference_transport_header.raw());
            let mut transport_header = transport_header_data.bind_mut(transport_header_raw);
            transport_header.swap_source_and_destination();
        }

        Self {
            buffer,
            transport_index,
            payload_index,
            ip_header_data,
            transport_header_data,
        }
    }

    pub fn packetize_empty_payload(&mut self) -> IpPacket<'_> {
        self.build(0)
    }

    pub fn packetize<R: DatagramReceiver>(&mut self, source: &mut R) -> io::Result<IpPacket<'_>> {
        let r = source.recv(&mut self.buffer[self.payload_index..])?;
        let ip_packet = self.build(r as u16);
        Ok(ip_packet)
    }

    /// Packetize from stream (`Read`) source.
    ///
    /// `Ok(Some(_))` when packet is available
    /// `Ok(None)` on EOF (read 0 byte)
    /// `Err(_)` on error
    pub fn packetize_read<R: io::Read>(
        &mut self,
        source: &mut R,
        max_chunk_size: Option<usize>,
    ) -> io::Result<Option<IpPacket<'_>>> {
        let mut adapter = ReadAdapter::new(source, max_chunk_size);
        let r = adapter.recv(&mut self.buffer[self.payload_index..])?;
        let option = if r > 0 {
            let ip_packet = self.build(r as u16);
            Some(ip_packet)
        } else {
            None
        };
        Ok(option)
    }

    pub fn ip_header_mut(&mut self) -> IpHeaderMut<'_> {
        let raw = &mut self.buffer[..self.transport_index];
        match &mut self.ip_header_data {
            IpHeaderData::V4(v4) => IpHeaderMut::V4(v4.bind_mut(raw)),
            IpHeaderData::V6(v6) => IpHeaderMut::V6(v6.bind_mut(raw)),
        }
    }

    pub fn transport_header_mut(&mut self) -> TransportHeaderMut<'_> {
        let raw = &mut self.buffer[self.transport_index..self.payload_index];
        self.transport_header_data.bind_mut(raw)
    }

    fn build(&mut self, payload_length: u16) -> IpPacket<'_> {
        let total_length = self.payload_index as u16 + payload_length;

        self.ip_header_mut().set_total_length(total_length);
        self.transport_header_mut()
            .set_payload_length(payload_length);

        let ip_data = self.ip_header_data.clone();

        match ip_data {
            IpHeaderData::V4(ref v4) => {
                let mut p = IpPacket::V4(Ipv4Packet::new(
                    &mut self.buffer[..total_length as usize],
                    v4.clone(),
                    self.transport_header_data.clone(),
                ));
                p.compute_checksums();
                p
            }
            IpHeaderData::V6(ref v6) => {
                let mut p = IpPacket::V6(Ipv6Packet::new(
                    &mut self.buffer[..total_length as usize],
                    v6.clone(),
                    self.transport_header_data.clone(),
                ));
                p.compute_checksums();
                p
            }
        }
    }

    pub fn inflate(&mut self, packet_length: u16) -> IpPacket<'_> {
        let ip_data = self.ip_header_data.clone();
        match ip_data {
            IpHeaderData::V4(ref v4) => IpPacket::V4(Ipv4Packet::new(
                &mut self.buffer[..packet_length as usize],
                v4.clone(),
                self.transport_header_data.clone(),
            )),
            IpHeaderData::V6(ref v6) => IpPacket::V6(Ipv6Packet::new(
                &mut self.buffer[..packet_length as usize],
                v6.clone(),
                self.transport_header_data.clone(),
            )),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::relay::datagram::tests::MockDatagramSocket;
    use crate::relay::ipv4_packet::Ipv4Packet as V4Packet;
    use byteorder::{BigEndian, WriteBytesExt};
    use std::io;

    fn create_v4_packet() -> Vec<u8> {
        let mut raw = Vec::new();
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
        raw
    }

    #[test]
    fn merge_headers_and_payload() {
        let raw = &mut create_v4_packet()[..];
        let reference_packet = V4Packet::parse(raw);

        let data = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let mut mock = MockDatagramSocket::from_data(&data);

        let ipv4_header = reference_packet.ipv4_header();
        let transport_header = reference_packet.transport_header().unwrap();
        let ip_header = IpHeader::V4(ipv4_header);
        let mut packetizer = Packetizer::new(&ip_header, &transport_header);

        let packet = packetizer.packetize(&mut mock).unwrap();
        match &packet {
            IpPacket::V4(p) => {
                assert_eq!(36, p.ipv4_header_data().total_length());
                assert_eq!(data, &p.raw()[28..36]);
            }
            _ => panic!("Expected V4 packet"),
        }
    }

    #[test]
    fn last_packet() {
        let raw = &mut create_v4_packet()[..];
        let reference_packet = V4Packet::parse(raw);

        let data = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let mut mock = MockDatagramSocket::from_data(&data);

        let ipv4_header = reference_packet.ipv4_header();
        let transport_header = reference_packet.transport_header().unwrap();
        let ip_header = IpHeader::V4(ipv4_header);
        let mut packetizer = Packetizer::new(&ip_header, &transport_header);

        let packet_length = match packetizer.packetize(&mut mock).unwrap() {
            IpPacket::V4(ref p) => p.length(),
            _ => panic!("Expected V4 packet"),
        };
        let packet = packetizer.inflate(packet_length);
        match &packet {
            IpPacket::V4(p) => {
                assert_eq!(36, p.ipv4_header_data().total_length());
                assert_eq!(data, &p.raw()[28..36]);
            }
            _ => panic!("Expected V4 packet"),
        }
    }

    #[test]
    fn packetize_chunks() {
        let raw = &mut create_v4_packet()[..];
        let reference_packet = V4Packet::parse(raw);

        let data = [0x11u8, 0x22, 0x33, 0x44, 0x55, 0x66, 0x77, 0x88];
        let mut cursor = io::Cursor::new(&data);

        let ipv4_header = reference_packet.ipv4_header();
        let transport_header = reference_packet.transport_header().unwrap();
        let ip_header = IpHeader::V4(ipv4_header);
        let mut packetizer = Packetizer::new(&ip_header, &transport_header);

        {
            let packet = packetizer
                .packetize_read(&mut cursor, Some(2))
                .unwrap()
                .unwrap();
            match &packet {
                IpPacket::V4(p) => {
                    assert_eq!(30, p.ipv4_header_data().total_length());
                    assert_eq!([0x11, 0x22], p.payload().unwrap());
                }
                _ => panic!("Expected V4 packet"),
            }
        }

        {
            let packet = packetizer
                .packetize_read(&mut cursor, Some(3))
                .unwrap()
                .unwrap();
            match &packet {
                IpPacket::V4(p) => {
                    assert_eq!(31, p.ipv4_header_data().total_length());
                    assert_eq!([0x33, 0x44, 0x55], p.payload().unwrap());
                }
                _ => panic!("Expected V4 packet"),
            }
        }

        {
            let packet = packetizer
                .packetize_read(&mut cursor, Some(1024))
                .unwrap()
                .unwrap();
            match &packet {
                IpPacket::V4(p) => {
                    assert_eq!(31, p.ipv4_header_data().total_length());
                    assert_eq!([0x66, 0x77, 0x88], p.payload().unwrap());
                }
                _ => panic!("Expected V4 packet"),
            }
        }
    }
}
