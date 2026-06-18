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

use std::net::IpAddr;

use super::ipv4_header::{Ipv4Header, Ipv4HeaderData, Ipv4HeaderMut, Protocol};
use super::ipv6_header::{Ipv6Header, Ipv6HeaderData, Ipv6HeaderMut};

// ── Data enum ──────────────────────────────────────────────────────────

#[derive(Clone)]
pub enum IpHeaderData {
    V4(Ipv4HeaderData),
    V6(Ipv6HeaderData),
}

#[allow(dead_code)]
impl IpHeaderData {
    pub fn protocol(&self) -> Protocol {
        match self {
            IpHeaderData::V4(v4) => v4.protocol(),
            IpHeaderData::V6(v6) => v6.protocol(),
        }
    }

    pub fn total_length(&self) -> u16 {
        match self {
            IpHeaderData::V4(v4) => v4.total_length(),
            IpHeaderData::V6(v6) => v6.total_length(),
        }
    }

    pub fn header_length(&self) -> u8 {
        match self {
            IpHeaderData::V4(v4) => v4.header_length(),
            IpHeaderData::V6(v6) => v6.header_length(),
        }
    }

    pub fn source(&self) -> IpAddr {
        match self {
            IpHeaderData::V4(v4) => IpAddr::V4(std::net::Ipv4Addr::from(v4.source().to_be_bytes())),
            IpHeaderData::V6(v6) => IpAddr::V6(v6.source()),
        }
    }

    pub fn destination(&self) -> IpAddr {
        match self {
            IpHeaderData::V4(v4) => {
                IpAddr::V4(std::net::Ipv4Addr::from(v4.destination().to_be_bytes()))
            }
            IpHeaderData::V6(v6) => IpAddr::V6(v6.destination()),
        }
    }
}

// ── Borrowed header enum ───────────────────────────────────────────────

pub enum IpHeader<'a> {
    V4(Ipv4Header<'a>),
    V6(Ipv6Header<'a>),
}

#[allow(dead_code)]
impl<'a> IpHeader<'a> {
    pub fn raw(&self) -> &[u8] {
        match self {
            IpHeader::V4(h) => h.raw(),
            IpHeader::V6(h) => h.raw(),
        }
    }

    pub fn protocol(&self) -> Protocol {
        match self {
            IpHeader::V4(h) => h.protocol(),
            IpHeader::V6(h) => h.protocol(),
        }
    }

    pub fn total_length(&self) -> u16 {
        match self {
            IpHeader::V4(h) => h.total_length(),
            IpHeader::V6(h) => h.total_length(),
        }
    }

    pub fn header_length(&self) -> u8 {
        match self {
            IpHeader::V4(h) => h.header_length(),
            IpHeader::V6(h) => h.header_length(),
        }
    }

    pub fn source(&self) -> IpAddr {
        match self {
            IpHeader::V4(h) => IpAddr::V4(std::net::Ipv4Addr::from(h.source().to_be_bytes())),
            IpHeader::V6(h) => IpAddr::V6(h.source()),
        }
    }

    pub fn destination(&self) -> IpAddr {
        match self {
            IpHeader::V4(h) => IpAddr::V4(std::net::Ipv4Addr::from(h.destination().to_be_bytes())),
            IpHeader::V6(h) => IpAddr::V6(h.destination()),
        }
    }

    /// Return the inner data as a cloneable IpHeaderData
    pub fn data_clone(&self) -> IpHeaderData {
        match self {
            IpHeader::V4(h) => IpHeaderData::V4(h.data().clone()),
            IpHeader::V6(h) => IpHeaderData::V6(h.data().clone()),
        }
    }
}

// ── Mutable header enum ────────────────────────────────────────────────

pub enum IpHeaderMut<'a> {
    V4(Ipv4HeaderMut<'a>),
    V6(Ipv6HeaderMut<'a>),
}

#[allow(dead_code)]
impl<'a> IpHeaderMut<'a> {
    pub fn raw(&self) -> &[u8] {
        match self {
            IpHeaderMut::V4(h) => h.raw(),
            IpHeaderMut::V6(h) => h.raw(),
        }
    }

    pub fn raw_mut(&mut self) -> &mut [u8] {
        match self {
            IpHeaderMut::V4(h) => h.raw_mut(),
            IpHeaderMut::V6(h) => h.raw_mut(),
        }
    }

    pub fn protocol(&self) -> Protocol {
        match self {
            IpHeaderMut::V4(h) => h.protocol(),
            IpHeaderMut::V6(h) => h.protocol(),
        }
    }

    pub fn total_length(&self) -> u16 {
        match self {
            IpHeaderMut::V4(h) => h.total_length(),
            IpHeaderMut::V6(h) => h.total_length(),
        }
    }

    pub fn header_length(&self) -> u8 {
        match self {
            IpHeaderMut::V4(h) => h.header_length(),
            IpHeaderMut::V6(h) => h.header_length(),
        }
    }

    pub fn source(&self) -> IpAddr {
        match self {
            IpHeaderMut::V4(h) => IpAddr::V4(std::net::Ipv4Addr::from(h.source().to_be_bytes())),
            IpHeaderMut::V6(h) => IpAddr::V6(h.source()),
        }
    }

    pub fn destination(&self) -> IpAddr {
        match self {
            IpHeaderMut::V4(h) => IpAddr::V4(std::net::Ipv4Addr::from(h.destination().to_be_bytes())),
            IpHeaderMut::V6(h) => IpAddr::V6(h.destination()),
        }
    }

    pub fn set_total_length(&mut self, total_length: u16) {
        match self {
            IpHeaderMut::V4(h) => h.set_total_length(total_length),
            IpHeaderMut::V6(h) => {
                let payload_length = total_length - u16::from(h.data().header_length());
                h.set_payload_length(payload_length);
            }
        }
    }

    pub fn swap_source_and_destination(&mut self) {
        match self {
            IpHeaderMut::V4(h) => h.swap_source_and_destination(),
            IpHeaderMut::V6(h) => h.swap_source_and_destination(),
        }
    }

    pub fn update_checksum(&mut self) {
        if let IpHeaderMut::V4(h) = self {
            h.update_checksum();
        }
        // IPv6 has no header checksum
    }

    /// Return the inner data as a cloneable IpHeaderData
    pub fn data_clone(&self) -> IpHeaderData {
        match self {
            IpHeaderMut::V4(h) => IpHeaderData::V4(h.data().clone()),
            IpHeaderMut::V6(h) => IpHeaderData::V6(h.data().clone()),
        }
    }
}
