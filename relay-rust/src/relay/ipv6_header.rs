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

use byteorder::{BigEndian, ByteOrder};
use std::net::Ipv6Addr;

use super::ipv4_header::Protocol;

pub const IPV6_HEADER_LENGTH: u8 = 40;

pub struct Ipv6Header<'a> {
    raw: &'a [u8],
    data: &'a Ipv6HeaderData,
}

pub struct Ipv6HeaderMut<'a> {
    raw: &'a mut [u8],
    data: &'a mut Ipv6HeaderData,
}

#[derive(Clone)]
pub struct Ipv6HeaderData {
    payload_length: u16,
    next_header: Protocol,
    hop_limit: u8,
    source: Ipv6Addr,
    destination: Ipv6Addr,
}

#[allow(dead_code)]
impl Ipv6HeaderData {
    pub fn parse(raw: &[u8]) -> Self {
        let mut source_bytes = [0u8; 16];
        let mut dest_bytes = [0u8; 16];
        source_bytes.copy_from_slice(&raw[8..24]);
        dest_bytes.copy_from_slice(&raw[24..40]);
        Self {
            payload_length: BigEndian::read_u16(&raw[4..6]),
            next_header: match raw[6] {
                6 => Protocol::Tcp,
                17 => Protocol::Udp,
                _ => Protocol::Other,
            },
            hop_limit: raw[7],
            source: Ipv6Addr::from(source_bytes),
            destination: Ipv6Addr::from(dest_bytes),
        }
    }

    pub fn bind<'c, 'a: 'c, 'b: 'c>(&'a self, raw: &'b [u8]) -> Ipv6Header<'c> {
        Ipv6Header::new(raw, self)
    }

    pub fn bind_mut<'c, 'a: 'c, 'b: 'c>(&'a mut self, raw: &'b mut [u8]) -> Ipv6HeaderMut<'c> {
        Ipv6HeaderMut::new(raw, self)
    }

    pub fn payload_length(&self) -> u16 {
        self.payload_length
    }

    pub fn total_length(&self) -> u16 {
        u16::from(IPV6_HEADER_LENGTH) + self.payload_length
    }

    pub fn header_length(&self) -> u8 {
        IPV6_HEADER_LENGTH
    }

    pub fn protocol(&self) -> Protocol {
        self.next_header
    }

    pub fn source(&self) -> Ipv6Addr {
        self.source
    }

    pub fn destination(&self) -> Ipv6Addr {
        self.destination
    }

    /// Return source as raw 16-byte array
    pub fn source_bytes(&self) -> [u8; 16] {
        self.source.octets()
    }

    /// Return destination as raw 16-byte array
    pub fn destination_bytes(&self) -> [u8; 16] {
        self.destination.octets()
    }

    pub fn hop_limit(&self) -> u8 {
        self.hop_limit
    }
}

// shared definition for Ipv6Header and Ipv6HeaderMut
macro_rules! ipv6_header_common {
    ($name:ident, $raw_type:ty, $data_type:ty) => {
        #[allow(dead_code)]
        impl<'a> $name<'a> {
            pub fn new(raw: $raw_type, data: $data_type) -> Self {
                Self { raw, data }
            }

            pub fn raw(&self) -> &[u8] {
                self.raw
            }

            pub fn data(&self) -> &Ipv6HeaderData {
                self.data
            }

            pub fn header_length(&self) -> u8 {
                IPV6_HEADER_LENGTH
            }

            pub fn payload_length(&self) -> u16 {
                self.data.payload_length
            }

            pub fn total_length(&self) -> u16 {
                self.data.total_length()
            }

            pub fn protocol(&self) -> Protocol {
                self.data.next_header
            }

            pub fn source(&self) -> Ipv6Addr {
                self.data.source
            }

            pub fn destination(&self) -> Ipv6Addr {
                self.data.destination
            }
        }
    };
}

ipv6_header_common!(Ipv6Header, &'a [u8], &'a Ipv6HeaderData);
ipv6_header_common!(Ipv6HeaderMut, &'a mut [u8], &'a mut Ipv6HeaderData);

// additional methods for the mutable version
#[allow(dead_code)]
impl<'a> Ipv6HeaderMut<'a> {
    pub fn raw_mut(&mut self) -> &mut [u8] {
        self.raw
    }

    pub fn data_mut(&mut self) -> &mut Ipv6HeaderData {
        self.data
    }

    pub fn set_payload_length(&mut self, payload_length: u16) {
        self.data.payload_length = payload_length;
        BigEndian::write_u16(&mut self.raw[4..6], payload_length);
    }

    pub fn set_source(&mut self, source: Ipv6Addr) {
        self.data.source = source;
        self.raw[8..24].copy_from_slice(&source.octets());
    }

    pub fn set_destination(&mut self, destination: Ipv6Addr) {
        self.data.destination = destination;
        self.raw[24..40].copy_from_slice(&destination.octets());
    }

    pub fn swap_source_and_destination(&mut self) {
        let src_octets = self.data.source.octets();
        let dst_octets = self.data.destination.octets();

        self.data.source = Ipv6Addr::from(dst_octets);
        self.data.destination = Ipv6Addr::from(src_octets);

        // swap the raw bytes (16-byte each)
        for i in 0..16 {
            self.raw.swap(8 + i, 24 + i);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use byteorder::{BigEndian, WriteBytesExt};

    fn create_header() -> Vec<u8> {
        let mut raw: Vec<u8> = Vec::new();
        raw.reserve(40);

        // Version (6), Traffic Class, Flow Label
        raw.write_u32::<BigEndian>(0x60000000).unwrap();
        // Payload Length (8 bytes of UDP + 4 bytes of payload = 12)
        raw.write_u16::<BigEndian>(12).unwrap();
        // Next Header (UDP)
        raw.write_u8(17).unwrap();
        // Hop Limit
        raw.write_u8(64).unwrap();
        // Source address: 2001:db8::1
        raw.extend_from_slice(&[
            0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x01,
        ]);
        // Destination address: 2001:db8::2
        raw.extend_from_slice(&[
            0x20, 0x01, 0x0d, 0xb8, 0x00, 0x00, 0x00, 0x00,
            0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x02,
        ]);

        raw
    }

    #[test]
    fn parse_header() {
        let raw = &create_header()[..];
        let data = Ipv6HeaderData::parse(raw);
        assert_eq!(12, data.payload_length);
        assert_eq!(52, data.total_length());
        assert_eq!(40, data.header_length());
        assert_eq!(Protocol::Udp, data.protocol());
        assert_eq!(
            Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 1),
            data.source()
        );
        assert_eq!(
            Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 2),
            data.destination()
        );
    }

    #[test]
    fn edit_header() {
        let raw = &mut create_header()[..];
        let mut header_data = Ipv6HeaderData::parse(raw);
        let mut header = header_data.bind_mut(raw);

        let new_src = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 3);
        let new_dst = Ipv6Addr::new(0x2001, 0x0db8, 0, 0, 0, 0, 0, 4);
        header.set_source(new_src);
        header.set_destination(new_dst);
        header.set_payload_length(42);

        assert_eq!(new_src, header.source());
        assert_eq!(new_dst, header.destination());
        assert_eq!(42, header.payload_length());

        // assert that the buffer has been modified
        let raw_src: [u8; 16] = header.raw[8..24].try_into().unwrap();
        let raw_dst: [u8; 16] = header.raw[24..40].try_into().unwrap();
        let raw_payload_length = BigEndian::read_u16(&header.raw[4..6]);
        assert_eq!(new_src.octets(), raw_src);
        assert_eq!(new_dst.octets(), raw_dst);
        assert_eq!(42, raw_payload_length);

        header.swap_source_and_destination();

        assert_eq!(new_dst, header.source());
        assert_eq!(new_src, header.destination());

        let raw_src: [u8; 16] = header.raw[8..24].try_into().unwrap();
        let raw_dst: [u8; 16] = header.raw[24..40].try_into().unwrap();
        assert_eq!(new_dst.octets(), raw_src);
        assert_eq!(new_src.octets(), raw_dst);
    }
}
