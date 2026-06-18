//! Defines the `Connection` trait and `ConnectionId` for tracking individual
//! TCP/UDP flows between the device and the internet.

use std::fmt;
use std::io;
use std::net::{IpAddr, Ipv4Addr, SocketAddr};

use super::client::ClientChannel;
use super::ipv4_header::Protocol;
use super::ip_header::IpHeaderData;
use super::ip_packet::IpPacket;
use super::net;
use super::transport_header::TransportHeaderData;

const LOCALHOST_FORWARD_V4: u32 = 0x0A_00_02_02; // 10.0.2.2

pub trait Connection {
    #[allow(dead_code)]
    fn id(&self) -> &ConnectionId;
    fn send_to_network(
        &mut self,
        client_channel: &mut ClientChannel,
        ip_packet: &IpPacket,
    );
    fn close(&mut self);
    fn is_expired(&self) -> bool;
    fn is_closed(&self) -> bool;
    /// Poll the connection for network I/O (non-blocking).
    /// Returns `WouldBlock` if no progress could be made.
    fn poll(&mut self) -> io::Result<()>;
}

#[derive(Clone, Debug, Hash, PartialEq, Eq)]
pub struct ConnectionId {
    protocol: Protocol,
    source_ip: IpAddr,
    source_port: u16,
    destination_ip: IpAddr,
    destination_port: u16,
    id_string: String,
}

impl ConnectionId {
    pub fn from_headers(
        ip_header_data: &IpHeaderData,
        transport_header_data: &TransportHeaderData,
    ) -> Self {
        let source_ip = ip_header_data.source();
        let source_port = transport_header_data.source_port();
        let destination_ip = ip_header_data.destination();
        let destination_port = transport_header_data.destination_port();
        let id_string = format!(
            "{} -> {}",
            net::to_socket_addr(source_ip, source_port),
            net::to_socket_addr(destination_ip, destination_port)
        );
        Self {
            protocol: ip_header_data.protocol(),
            source_ip,
            source_port,
            destination_ip,
            destination_port,
            id_string,
        }
    }

    #[inline]
    pub fn protocol(&self) -> Protocol {
        self.protocol
    }

    pub fn rewritten_destination(&self) -> SocketAddr {
        // IPv4-only forwarding: 10.0.2.2 -> 127.0.0.1 (Android emulator)
        let ip = match self.destination_ip {
            IpAddr::V4(v4) => {
                let raw = v4.octets();
                let v4_raw = u32::from_be_bytes(raw);
                if v4_raw == LOCALHOST_FORWARD_V4 {
                    IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1))
                } else {
                    self.destination_ip
                }
            }
            _ => self.destination_ip,
        };
        SocketAddr::new(ip, self.destination_port)
    }
}

impl fmt::Display for ConnectionId {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        write!(f, "{}", self.id_string)
    }
}

// macros to log connection id along with the message

macro_rules! cx_format {
    ($id:tt, $str:tt, $($arg:tt)+) => {
        format!(concat!("{} ", $str), $id, $($arg)+)
    };
    ($id:tt, $str:tt) => {
        format!(concat!("{} ", $str), $id)
    };
}

macro_rules! cx_trace {
    (target: $target:expr, $id:expr, $($arg:tt)*) => {
        log::trace!(target: $target, "{}", cx_format!($id, $($arg)+))
    }
}

macro_rules! cx_debug {
    (target: $target:expr, $id:expr, $($arg:tt)*) => {
        log::debug!(target: $target, "{}", cx_format!($id, $($arg)+))
    }
}

macro_rules! cx_info {
    (target: $target:expr, $id:expr, $($arg:tt)*) => {
        log::info!(target: $target, "{}", cx_format!($id, $($arg)+))
    }
}

macro_rules! cx_warn {
    (target: $target:expr, $id:expr, $($arg:tt)*) => {
        log::warn!(target: $target, "{}", cx_format!($id, $($arg)+))
    }
}

macro_rules! cx_error {
    (target: $target:expr, $id:expr, $($arg:tt)*) => {
        log::error!(target: $target, "{}", cx_format!($id, $($arg)+))
    }
}
