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

use super::ip_header::{IpHeader, IpHeaderData};
use super::ipv4_packet::Ipv4Packet;
use super::ipv6_header::{Ipv6HeaderData, IPV6_HEADER_LENGTH};
use super::transport_header::{TransportHeader, TransportHeaderData};

/// Enum that holds either an IPv4 or IPv6 packet.
///
/// Each variant owns the raw buffer reference plus the parsed header data,
/// following the same pattern as `Ipv4Packet`.
pub enum IpPacket<'a> {
    V4(Ipv4Packet<'a>),
    V6(Ipv6Packet<'a>),
}

/// Internal representation of a parsed IPv6 packet.
pub struct Ipv6Packet<'a> {
    raw: &'a mut [u8],
    ipv6_header_data: Ipv6HeaderData,
    transport_header_data: Option<TransportHeaderData>,
}

impl<'a> Ipv6Packet<'a> {
    pub fn parse(raw: &'a mut [u8]) -> Self {
        let ipv6_header_data = Ipv6HeaderData::parse(raw);
        let total_len = ipv6_header_data.total_length() as usize;
        let transport_header_data = {
            let payload = &raw[IPV6_HEADER_LENGTH as usize..total_len];
            TransportHeaderData::parse(ipv6_header_data.protocol(), payload)
        };
        Self {
            raw: &mut raw[..total_len],
            ipv6_header_data,
            transport_header_data,
        }
    }

    pub fn new(
        raw: &'a mut [u8],
        ipv6_header_data: Ipv6HeaderData,
        transport_header_data: TransportHeaderData,
    ) -> Self {
        Self {
            raw,
            ipv6_header_data,
            transport_header_data: Some(transport_header_data),
        }
    }

    #[inline]
    pub fn raw(&self) -> &[u8] {
        self.raw
    }

    #[inline]
    pub fn headers_data(&self) -> (&Ipv6HeaderData, Option<&TransportHeaderData>) {
        (&self.ipv6_header_data, self.transport_header_data.as_ref())
    }

    pub fn headers(&self) -> (super::ipv6_header::Ipv6Header<'_>, Option<TransportHeader<'_>>) {
        if let Some(ref transport_header_data) = self.transport_header_data {
            let (ipv6_header_slice, transport_slice) =
                self.raw.split_at(IPV6_HEADER_LENGTH as usize);
            let payload_index = transport_header_data.header_length() as usize;
            let transport_header_slice = &transport_slice[..payload_index];
            let ipv6_header = self.ipv6_header_data.bind(ipv6_header_slice);
            let transport_header = transport_header_data.bind(transport_header_slice);
            (ipv6_header, Some(transport_header))
        } else {
            let ipv6_header_slice = &self.raw[..IPV6_HEADER_LENGTH as usize];
            let ipv6_header = self.ipv6_header_data.bind(ipv6_header_slice);
            (ipv6_header, None)
        }
    }

    #[inline]
    pub fn transport_header(&self) -> Option<TransportHeader<'_>> {
        self.transport_header_data.as_ref().map(|transport_header_data| {
            let start = IPV6_HEADER_LENGTH as usize;
            let end = start + transport_header_data.header_length() as usize;
            let slice = &self.raw[start..end];
            transport_header_data.bind(slice)
        })
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        self.transport_header_data.is_some()
    }

    #[inline]
    pub fn length(&self) -> u16 {
        self.ipv6_header_data.total_length()
    }

    pub fn payload(&self) -> Option<&[u8]> {
        self.transport_header_data.as_ref().map(|transport_header_data| {
            let range = IPV6_HEADER_LENGTH as usize
                + transport_header_data.header_length() as usize..;
            &self.raw[range]
        })
    }

    pub fn compute_checksums(&mut self) {
        // IPv6 has no header checksum; only compute transport checksums
        if let Some((mut transport_header, payload)) = {
            if let Some(ref mut transport_header_data) = self.transport_header_data {
                let payload_index = transport_header_data.header_length() as usize;
                let (ipv6_header_slice, transport_slice) =
                    self.raw.split_at_mut(IPV6_HEADER_LENGTH as usize);
                let (transport_header_slice, payload_slice) =
                    transport_slice.split_at_mut(payload_index);
                let _ipv6_header = self.ipv6_header_data.bind_mut(ipv6_header_slice);
                let transport_header = transport_header_data.bind_mut(transport_header_slice);
                Some((transport_header, payload_slice))
            } else {
                None
            }
        } {
            let ip_data = IpHeaderData::V6(self.ipv6_header_data.clone());
            transport_header.update_checksum(&ip_data, payload);
        }
    }
}

// ── IpPacket methods ───────────────────────────────────────────────────

#[allow(dead_code)]
impl<'a> IpPacket<'a> {
    pub fn parse(raw: &'a mut [u8]) -> Option<Self> {
        if raw.is_empty() {
            return None;
        }
        let version = raw[0] >> 4;
        match version {
            4 => Some(IpPacket::V4(Ipv4Packet::parse(raw))),
            6 => Some(IpPacket::V6(Ipv6Packet::parse(raw))),
            _ => None,
        }
    }

    #[inline]
    pub fn raw(&self) -> &[u8] {
        match self {
            IpPacket::V4(p) => p.raw(),
            IpPacket::V6(p) => p.raw(),
        }
    }

    #[inline]
    pub fn headers_data(&self) -> (IpHeaderData, Option<&TransportHeaderData>) {
        match self {
            IpPacket::V4(p) => {
                let (v4, t) = p.headers_data();
                (IpHeaderData::V4(v4.clone()), t)
            }
            IpPacket::V6(p) => {
                let (v6, t) = p.headers_data();
                (IpHeaderData::V6(v6.clone()), t)
            }
        }
    }

    pub fn headers(&self) -> (IpHeader<'_>, Option<TransportHeader<'_>>) {
        match self {
            IpPacket::V4(p) => {
                let (v4, t) = p.headers();
                (IpHeader::V4(v4), t)
            }
            IpPacket::V6(p) => {
                let (v6, t) = p.headers();
                (IpHeader::V6(v6), t)
            }
        }
    }

    #[inline]
    pub fn is_valid(&self) -> bool {
        match self {
            IpPacket::V4(p) => p.is_valid(),
            IpPacket::V6(p) => p.is_valid(),
        }
    }

    #[inline]
    pub fn length(&self) -> u16 {
        match self {
            IpPacket::V4(p) => p.length(),
            IpPacket::V6(p) => p.length(),
        }
    }

    pub fn payload(&self) -> Option<&[u8]> {
        match self {
            IpPacket::V4(p) => p.payload(),
            IpPacket::V6(p) => p.payload(),
        }
    }

    pub fn transport_header(&self) -> Option<TransportHeader<'_>> {
        match self {
            IpPacket::V4(p) => p.transport_header(),
            IpPacket::V6(p) => p.transport_header(),
        }
    }

    pub fn compute_checksums(&mut self) {
        match self {
            IpPacket::V4(p) => p.compute_checksums(),
            IpPacket::V6(p) => p.compute_checksums(),
        }
    }
}

impl<'a> From<Ipv4Packet<'a>> for IpPacket<'a> {
    fn from(p: Ipv4Packet<'a>) -> Self {
        IpPacket::V4(p)
    }
}
