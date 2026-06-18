//! Manages an individual UDP "connection" (flow) between the relay and an
//! internet server. UDP is connectionless, but we track state per remote host.

use log::*;
use socket2::SockRef;
use std::cell::RefCell;
use std::io;
use std::net::{SocketAddr, UdpSocket};
use std::rc::{Rc, Weak};
use std::time::Instant;

use super::binary;
use super::client::{Client, ClientChannel};
use super::connection::{Connection, ConnectionId};
use super::datagram_buffer::DatagramBuffer;
use super::ip_header::IpHeader;
use super::ip_packet::IpPacket;
use super::ipv4_packet::MAX_PACKET_LENGTH;
use super::packetizer::Packetizer;
use super::transport_header::TransportHeader;

const TAG: &str = "UdpConnection";

// Priority 6: Increased from 60 to 300 seconds to reduce cleanup churn
pub const IDLE_TIMEOUT_SECONDS: u64 = 300;

/// Bandwidth tracking for UDP.
#[allow(dead_code)]
pub static mut GLOBAL_BYTES_SENT: u64 = 0;
#[allow(dead_code)]
pub static mut GLOBAL_BYTES_RECEIVED: u64 = 0;

pub struct UdpConnection {
    id: ConnectionId,
    client: Weak<RefCell<Client>>,
    socket: UdpSocket,
    client_to_network: DatagramBuffer,
    network_to_client: Packetizer,
    closed: bool,
    idle_since: Instant,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

impl UdpConnection {
    #[allow(clippy::needless_pass_by_value)] // semantically, headers are consumed
    pub fn create(
        id: ConnectionId,
        client: Weak<RefCell<Client>>,
        ip_header: IpHeader,
        transport_header: TransportHeader,
    ) -> io::Result<Rc<RefCell<Self>>> {
        cx_info!(target: TAG, id, "Open");
        let socket = Self::create_socket(&id)?;
        let packetizer = Packetizer::new(&ip_header, &transport_header);
        let rc = Rc::new(RefCell::new(Self {
            id,
            client,
            socket,
            client_to_network: DatagramBuffer::new(4 * MAX_PACKET_LENGTH),
            network_to_client: packetizer,
            closed: false,
            idle_since: Instant::now(),
            bytes_sent: 0,
            bytes_received: 0,
        }));
        Ok(rc)
    }

    fn create_socket(id: &ConnectionId) -> io::Result<UdpSocket> {
        let autobind_addr = SocketAddr::from((std::net::Ipv6Addr::UNSPECIFIED, 0));
        let udp_socket = UdpSocket::bind(autobind_addr)?;
        udp_socket.connect(id.rewritten_destination())?;
        udp_socket.set_nonblocking(true)?;

        // Priority 6: Increase SO_RCVBUF to 2MB for better UDP throughput
        let sock_ref = SockRef::from(&udp_socket);
        let _ = sock_ref.set_recv_buffer_size(2 * 1024 * 1024);

        Ok(udp_socket)
    }

    fn remove_from_router(&self) {
        let client_rc = match self.client.upgrade() {
            Some(c) => c,
            None => {
                warn!(target: TAG, "Client already dropped, cannot remove from router");
                return;
            }
        };
        let mut client = client_rc.borrow_mut();
        client.router().remove(&self.id);
    }

    /// Poll the connection: try to send/receive on the network socket.
    /// Returns `WouldBlock` if no progress could be made.
    fn poll_self(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }

        self.touch();
        let mut made_progress = false;

        // Try to send data to the network
        if !self.client_to_network.is_empty() {
            match self.process_send() {
                Ok(()) => made_progress = true,
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                    cx_debug!(target: TAG, self.id, "Spurious event, ignoring")
                }
                Err(err) => {
                    cx_error!(
                        target: TAG,
                        self.id,
                        "Cannot write: [{:?}] {}",
                        err.kind(),
                        err
                    );
                    self.close();
                    return Ok(());
                }
            }
        }

        // Try to read data from the network
        if !self.closed {
            match self.process_receive() {
                Ok(()) => made_progress = true,
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(err) => {
                    cx_error!(
                        target: TAG,
                        self.id,
                        "Cannot read: [{:?}] {}",
                        err.kind(),
                        err
                    );
                    self.close();
                    return Ok(());
                }
            }
        }

        if self.closed {
            self.remove_from_router();
        }

        if made_progress {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::WouldBlock,
                "Connection would block",
            ))
        }
    }

    fn process_send(&mut self) -> io::Result<()> {
        match self.write() {
            Ok(_) => Ok(()),
            Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {
                cx_debug!(target: TAG, self.id, "Spurious event, ignoring");
                Err(io::Error::new(io::ErrorKind::WouldBlock, "Would block"))
            }
            Err(err) => Err(err),
        }
    }

    fn process_receive(&mut self) -> io::Result<()> {
        match self.read() {
            Ok(_) => Ok(()),
            Err(err) => Err(err),
        }
    }

    fn read(&mut self) -> io::Result<()> {
        let ip_packet = self.network_to_client.packetize(&mut self.socket)?;
        self.bytes_received += ip_packet.payload().map(|p| p.len() as u64).unwrap_or(0);
        let client_rc = match self.client.upgrade() {
            Some(c) => c,
            None => {
                warn!(target: TAG, "Client already dropped, cannot send UDP packet");
                return Ok(());
            }
        };
        match client_rc
            .borrow_mut()
            .send_to_client(&ip_packet)
        {
            Ok(_) => {
                cx_debug!(
                    target: TAG,
                    self.id,
                    "Packet ({} bytes) sent to client",
                    ip_packet.length()
                );
                if log_enabled!(target: TAG, Level::Trace) {
                    cx_trace!(
                        target: TAG,
                        self.id,
                        "{}",
                        binary::build_packet_string(ip_packet.raw())
                    );
                }
            }
            Err(_) => cx_warn!(target: TAG, self.id, "Cannot send to client, drop packet"),
        }
        Ok(())
    }

    fn write(&mut self) -> io::Result<()> {
        self.client_to_network.write_to(&mut self.socket)?;
        Ok(())
    }

    fn touch(&mut self) {
        self.idle_since = Instant::now();
    }
}

impl Connection for UdpConnection {
    fn id(&self) -> &ConnectionId {
        &self.id
    }

    fn send_to_network(
        &mut self,
        _: &mut ClientChannel,
        ip_packet: &IpPacket,
    ) {
        if let Some(payload) = ip_packet.payload() {
            self.bytes_sent += payload.len() as u64;
            match self.client_to_network.read_from(payload) {
                Ok(_) => {}
                Err(err) => cx_warn!(
                    target: TAG,
                    self.id,
                    "Cannot send to network, drop packet: {}",
                    err
                ),
            }
        }
    }

    fn close(&mut self) {
        cx_info!(target: TAG, self.id, "Close");
        self.closed = true;
    }

    fn is_expired(&self) -> bool {
        self.idle_since.elapsed().as_secs() > IDLE_TIMEOUT_SECONDS
    }

    fn is_closed(&self) -> bool {
        self.closed
    }

    fn poll(&mut self) -> io::Result<()> {
        self.poll_self()
    }
}
