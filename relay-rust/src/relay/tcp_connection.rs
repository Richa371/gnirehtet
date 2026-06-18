//! Manages an individual TCP connection between the relay and an internet server.
//! Implements a TCP state machine to translate between the device's virtual TCP
//! and the real TCP socket on the host.

use log::*;
use rand::random;
use socket2::{SockRef, TcpKeepalive};
use std::cell::RefCell;
use std::cmp;
use std::io::{self, Write};
use std::net::TcpStream;
use std::num::Wrapping;
use std::rc::{Rc, Weak};
use std::time::Duration;

use super::binary;
use super::client::{Client, ClientChannel};
use super::connection::{Connection, ConnectionId};
use super::ip_header::IpHeader;
use super::ip_packet::IpPacket;
use super::ipv4_packet::MAX_PACKET_LENGTH;
use super::packet_source::PacketSource;
use super::packetizer::Packetizer;
use super::stream_buffer::StreamBuffer;
use super::tcp_header::{self, TcpHeader, TcpHeaderMut};
use super::transport_header::{TransportHeader, TransportHeaderMut};
use std::sync::OnceLock;

const TAG: &str = "TcpConnection";

/// Global SOCKS5 proxy address, set at startup by the CLI `--socks5` flag.
pub static SOCKS5_PROXY: OnceLock<std::net::SocketAddr> = OnceLock::new();

const MTU: u16 = 0x4000;
const MAX_PAYLOAD_LENGTH: u16 = MTU - 20 - 20_u16;

#[allow(dead_code)]
pub static mut GLOBAL_BYTES_SENT: u64 = 0;
#[allow(dead_code)]
pub static mut GLOBAL_BYTES_RECEIVED: u64 = 0;

pub struct TcpConnection {
    self_weak: Weak<RefCell<TcpConnection>>,
    id: ConnectionId,
    client: Weak<RefCell<Client>>,
    stream: TcpStream,
    client_to_network: StreamBuffer,
    network_to_client: Packetizer,
    packet_for_client_length: Option<u16>,
    closed: bool,
    tcb: Tcb,
    pub bytes_sent: u64,
    pub bytes_received: u64,
}

struct Tcb {
    state: TcpState,
    syn_sequence_number: u32,
    sequence_number: Wrapping<u32>,
    acknowledgement_number: Wrapping<u32>,
    their_acknowledgement_number: u32,
    fin_sequence_number: Option<u32>,
    fin_received: bool,
    client_window: u16,
}

#[derive(Debug, PartialEq, Eq)]
enum TcpState {
    Init,
    SynSent,
    SynReceived,
    Established,
    LastAck,
    Closing,
    FinWait1,
    FinWait2,
}

impl TcpState {
    #[inline]
    fn is_connected(&self) -> bool {
        !matches!(self, TcpState::Init | TcpState::SynSent | TcpState::SynReceived)
    }

    #[inline]
    fn is_closed(&self) -> bool {
        matches!(
            self,
            TcpState::FinWait1 | TcpState::FinWait2 | TcpState::Closing | TcpState::LastAck
        )
    }
}

impl Tcb {
    fn new() -> Self {
        Self {
            state: TcpState::Init,
            syn_sequence_number: 0,
            sequence_number: Wrapping(0),
            acknowledgement_number: Wrapping(0),
            their_acknowledgement_number: 0,
            fin_sequence_number: None,
            fin_received: false,
            client_window: 0,
        }
    }

    #[inline]
    fn remaining_client_window(&self) -> u16 {
        let wrapped_remaining = Wrapping(self.their_acknowledgement_number)
            + Wrapping(u32::from(self.client_window))
            - self.sequence_number;
        let remaining = wrapped_remaining.0;
        if remaining <= u32::from(self.client_window) {
            remaining as u16
        } else {
            0
        }
    }

    fn numbers(&self) -> String {
        format!(
            "(seq={}, ack={})",
            self.sequence_number, self.acknowledgement_number
        )
    }
}

impl TcpConnection {
    #[allow(clippy::needless_pass_by_value)] // semantically, headers are consumed
    pub fn create(
        id: ConnectionId,
        client: Weak<RefCell<Client>>,
        ip_header: IpHeader,
        transport_header: TransportHeader,
    ) -> io::Result<Rc<RefCell<Self>>> {
        cx_info!(target: TAG, id, "Open");
        let stream = Self::create_stream(&id)?;

        let tcp_header = Self::tcp_header_of_transport(transport_header);

        // shrink the TCP options to pass a minimal refrence header to the packetizer
        let mut shrinked_tcp_header_raw = [0u8; 20];
        shrinked_tcp_header_raw.copy_from_slice(&tcp_header.raw()[..20]);
        let mut shrinked_tcp_header_data = tcp_header.data().clone();
        {
            let mut shrinked_tcp_header =
                shrinked_tcp_header_data.bind_mut(&mut shrinked_tcp_header_raw);
            shrinked_tcp_header.shrink_options();
            debug_assert_eq!(20, shrinked_tcp_header.header_length());
        }

        let shrinked_transport_header = shrinked_tcp_header_data
            .bind(&shrinked_tcp_header_raw)
            .into();

        let packetizer = Packetizer::new(&ip_header, &shrinked_transport_header);

        let rc = Rc::new(RefCell::new(Self {
            self_weak: Weak::new(),
            id,
            client,
            stream,
            client_to_network: StreamBuffer::new(4 * MAX_PACKET_LENGTH),
            network_to_client: packetizer,
            packet_for_client_length: None,
            closed: false,
            tcb: Tcb::new(),
            bytes_sent: 0,
            bytes_received: 0,
        }));

        {
            let mut self_ref = rc.borrow_mut();
            self_ref.self_weak = Rc::downgrade(&rc);
        }
        Ok(rc)
    }

    fn create_stream(id: &ConnectionId) -> io::Result<TcpStream> {
        let dest = id.rewritten_destination();
        let stream = if let Some(proxy) = SOCKS5_PROXY.get() {
            Self::connect_via_socks5(proxy, &dest)?
        } else {
            TcpStream::connect(dest)?
        };
        stream.set_nonblocking(true)?;
        stream.set_nodelay(true)?;

        let sock_ref = SockRef::from(&stream);
        let _ = sock_ref.set_recv_buffer_size(1024 * 1024);
        let _ = sock_ref.set_send_buffer_size(256 * 1024);

        let ka = TcpKeepalive::new().with_time(Duration::from_secs(60));
        let _ = sock_ref.set_tcp_keepalive(&ka);

        #[cfg(target_os = "linux")]
        {
            use std::os::fd::AsRawFd;
            let fd = stream.as_raw_fd();
            let one: libc::c_int = 1;
            unsafe {
                libc::setsockopt(
                    fd,
                    libc::SOL_TCP,
                    libc::TCP_FASTOPEN_CONNECT,
                    &one as *const _ as *const libc::c_void,
                    std::mem::size_of::<libc::c_int>() as libc::socklen_t,
                );
            }
        }

        Ok(stream)
    }

    /// Connect to a destination through a SOCKS5 proxy.
    fn connect_via_socks5(proxy: &std::net::SocketAddr, destination: &std::net::SocketAddr) -> io::Result<TcpStream> {
        use std::io::{Read, Write};
        let mut stream = TcpStream::connect(proxy)?;
        stream.set_nonblocking(false)?;

        let greeting = [0x05, 0x01, 0x00];
        stream.write_all(&greeting)?;

        let mut response = [0u8; 2];
        stream.read_exact(&mut response)?;
        if response != [0x05, 0x00] {
            return Err(io::Error::other(
                format!("SOCKS5 handshake failed: expected [0x05, 0x00], got {:?}", response)));
        }

        let ip_bytes = match destination.ip() {
            std::net::IpAddr::V4(ip) => ip.octets().to_vec(),
            std::net::IpAddr::V6(_ip) => return Err(io::Error::new(io::ErrorKind::Unsupported,
                "SOCKS5 proxy does not support IPv6 destinations")),
        };
        let port_be = destination.port().to_be_bytes();
        let mut connect_request = vec![0x05, 0x01, 0x00, 0x01];
        connect_request.extend_from_slice(&ip_bytes);
        connect_request.extend_from_slice(&port_be);
        stream.write_all(&connect_request)?;

        let mut reply = [0u8; 4];
        stream.read_exact(&mut reply)?;
        if reply[0] != 0x05 || reply[1] != 0x00 {
            return Err(io::Error::other(
                format!("SOCKS5 connect failed: reply={:?}", reply)));
        }
        let addr_type = reply[3];
        let remaining_len = match addr_type {
            0x01 => 4 + 2,
            0x03 => {
                let mut len_byte = [0u8; 1];
                stream.read_exact(&mut len_byte)?;
                len_byte[0] as usize + 1 + 2
            }
            0x04 => 16 + 2,
            _ => return Err(io::Error::other(
                format!("SOCKS5 unknown address type: {}", addr_type))),
        };
        if remaining_len > 0 {
            let mut rest = vec![0u8; remaining_len];
            stream.read_exact(&mut rest)?;
        }

        stream.set_nonblocking(true)?;
        Ok(stream)
    }

    fn remove_from_router(&self) {
        let client_rc = self.client.upgrade().unwrap_or_else(|| panic!("Expected client not found"));
        let mut client = client_rc.borrow_mut();
        client.router().remove(&self.id);
    }

    /// Poll the connection: try to send/receive on the network socket.
    fn poll_self(&mut self) -> io::Result<()> {
        if self.closed {
            return Ok(());
        }

        let mut made_progress = false;

        if self.may_write() {
            match self.process_send() {
                Ok(()) => made_progress = true,
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(err) => {
                    cx_error!(
                        target: TAG,
                        self.id,
                        "Cannot write: [{:?}] {}",
                        err.kind(),
                        err
                    );
                    self.send_empty_packet_to_client(tcp_header::FLAG_RST);
                    self.close();
                    return Ok(());
                }
            }
        }

        if !self.closed && self.may_read() {
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
                    self.send_empty_packet_to_client(tcp_header::FLAG_RST);
                    self.close();
                    return Ok(());
                }
            }
        }

        if self.tcb.state == TcpState::SynSent {
            match self.stream.write(b"") {
                Ok(_) => {
                    self.process_connect();
                    made_progress = true;
                }
                Err(ref err) if err.kind() == io::ErrorKind::WouldBlock => {}
                Err(err) => {
                    cx_error!(
                        target: TAG,
                        self.id,
                        "Cannot connect: [{:?}] {}",
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
        match self.client_to_network.write_to(&mut self.stream) {
            Ok(w) => {
                if w != 0 {
                    self.bytes_sent += w as u64;
                    self.tcb.acknowledgement_number += Wrapping(w as u32);

                    if self.tcb.fin_received && self.client_to_network.is_empty() {
                        let Some(client_rc) = self.client.upgrade() else {
                            return Ok(());
                        };
                        let mut client = client_rc.borrow_mut();
                        cx_debug!(
                            target: TAG,
                            self.id,
                            "No more pending data, process the pending FIN"
                        );
                        self.do_handle_fin(&mut client.channel());
                    } else {
                        cx_debug!(
                            target: TAG,
                            self.id,
                            "Sending ACK {} to client",
                            self.tcb.numbers()
                        );
                        self.send_empty_packet_to_client(tcp_header::FLAG_ACK);
                    }
                } else {
                    self.close();
                }
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::WouldBlock {
                    return Err(err);
                }
                cx_error!(
                    target: TAG,
                    self.id,
                    "Cannot write: [{:?}] {}",
                    err.kind(),
                    err
                );
                self.send_empty_packet_to_client(tcp_header::FLAG_RST);
                self.close();
            }
        }
        Ok(())
    }

    fn process_receive(&mut self) -> io::Result<()> {
        debug_assert!(
            self.packet_for_client_length.is_none(),
            "A pending packet was not sent"
        );
        let remaining_client_window = self.tcb.remaining_client_window();
        debug_assert!(
            remaining_client_window > 0,
            "process_received() must not be called when window == 0"
        );
        let max_payload_length =
            Some(cmp::min(remaining_client_window, MAX_PAYLOAD_LENGTH) as usize);
        Self::update_headers(
            &mut self.network_to_client,
            &self.tcb,
            tcp_header::FLAG_ACK | tcp_header::FLAG_PSH,
        );
        match self
            .network_to_client
            .packetize_read(&mut self.stream, max_payload_length)
        {
            Ok(Some(ip_packet)) => {
                self.bytes_received += ip_packet.payload().map(|p| p.len() as u64).unwrap_or(0);
                match Self::send_to_client(&self.client, &ip_packet) {
                    Ok(_) => {
                        let len = ip_packet.payload().unwrap().len();
                        cx_debug!(
                            target: TAG,
                            self.id,
                            "Packet ({} bytes) sent to client {}",
                            len,
                            self.tcb.numbers()
                        );
                        self.tcb.sequence_number += Wrapping(len as u32);
                    }
                    Err(_) => {
                        let client_rc = match self.client.upgrade() {
                            Some(c) => c,
                            None => {
                                warn!(target: TAG, "Client already dropped, closing stale connection");
                                self.close();
                                return Ok(());
                            }
                        };
                        let mut client = client_rc.borrow_mut();
                        let self_rc = self.self_weak.upgrade().unwrap();
                        client.register_pending_packet_source(self_rc);
                        self.packet_for_client_length = Some(ip_packet.length());
                    }
                };
            }
            Ok(None) => {
                self.eof();
            }
            Err(err) => {
                if err.kind() == io::ErrorKind::WouldBlock {
                    return Err(err);
                }
                cx_error!(
                    target: TAG,
                    self.id,
                    "Cannot read: [{:?}] {}",
                    err.kind(),
                    err
                );
                self.send_empty_packet_to_client(tcp_header::FLAG_RST);
                self.close();
            }
        }
        Ok(())
    }

    fn process_connect(&mut self) {
        debug_assert_eq!(self.tcb.state, TcpState::SynSent);
        self.tcb.state = TcpState::SynReceived;
        cx_debug!(target: TAG, self.id, "State = {:?}", self.tcb.state);
        self.send_empty_packet_to_client(tcp_header::FLAG_SYN | tcp_header::FLAG_ACK);
        self.tcb.sequence_number += Wrapping(1);
    }

    fn send_to_client(
        client: &Weak<RefCell<Client>>,
        ip_packet: &IpPacket,
    ) -> io::Result<()> {
        let client_rc = match client.upgrade() {
            Some(c) => c,
            None => return Err(io::Error::new(io::ErrorKind::NotConnected, "client dropped")),
        };
        let mut client = client_rc.borrow_mut();
        client.send_to_client(ip_packet)
    }

    /// Send empty packet with the given flags to the client.
    fn send_empty_packet_to_client(&mut self, flags: u16) {
        let client_rc = match self.client.upgrade() {
            Some(c) => c,
            None => {
                warn!(target: TAG, "Client already dropped, closing stale connection");
                self.close();
                return;
            }
        };
        let mut client = client_rc.borrow_mut();
        self.reply_empty_packet_to_client(&mut client.channel(), flags);
    }

    fn reply_empty_packet_to_client(
        &mut self,
        client_channel: &mut ClientChannel,
        flags: u16,
    ) {
        let ip_packet =
            Self::create_empty_response_packet(&self.id, &mut self.network_to_client, &self.tcb, flags);
        let _ = client_channel.send_to_client(&ip_packet);
    }

    fn eof(&mut self) {
        let client_rc = match self.client.upgrade() {
            Some(c) => c,
            None => {
                warn!(target: TAG, "Client already dropped, closing stale connection");
                self.close();
                return;
            }
        };
        let mut client = client_rc.borrow_mut();
        cx_debug!(target: TAG, self.id, "EOF");
        self.tcb.acknowledgement_number += Wrapping(1); // FIN counts for 1 byte
        self.reply_empty_packet_to_client(&mut client.channel(), tcp_header::FLAG_FIN | tcp_header::FLAG_ACK);
        self.tcb.fin_sequence_number = Some(self.tcb.sequence_number.0);
        self.tcb.sequence_number += Wrapping(1); // FIN counts for 1 byte
        self.tcb.state = TcpState::FinWait1;
        cx_debug!(target: TAG, self.id, "State = {:?}", self.tcb.state);
    }

    #[inline]
    fn tcp_header_of_transport(transport_header: TransportHeader) -> TcpHeader {
        if let TransportHeader::Tcp(tcp_header) = transport_header {
            tcp_header
        } else {
            panic!("Not a TCP header");
        }
    }

    #[inline]
    fn tcp_header_of_transport_mut(transport_header: TransportHeaderMut) -> TcpHeaderMut {
        if let TransportHeaderMut::Tcp(tcp_header) = transport_header {
            tcp_header
        } else {
            panic!("Not a TCP header");
        }
    }

    #[inline]
    fn tcp_header_of_packet<'a>(ip_packet: &'a IpPacket) -> TcpHeader<'a> {
        match ip_packet.transport_header() {
            Some(TransportHeader::Tcp(tcp_header)) => tcp_header,
            _ => panic!("Not a TCP packet"),
        }
    }

    fn update_headers(packetizer: &mut Packetizer, tcb: &Tcb, flags: u16) {
        let mut tcp_header = Self::tcp_header_of_transport_mut(packetizer.transport_header_mut());
        tcp_header.set_sequence_number(tcb.sequence_number.0);
        tcp_header.set_acknowledgement_number(tcb.acknowledgement_number.0);
        tcp_header.set_flags(flags);
    }

    fn handle_packet(
        &mut self,
        client_channel: &mut ClientChannel,
        ip_packet: &IpPacket,
    ) {
        let tcp_header = Self::tcp_header_of_packet(ip_packet);
        if self.tcb.state == TcpState::Init {
            self.handle_first_packet(client_channel, ip_packet);
            return;
        }

        if tcp_header.is_syn() {
            self.handle_duplicate_syn(client_channel, ip_packet);
            return;
        }

        let expected_packet =
            (self.tcb.acknowledgement_number + Wrapping(self.client_to_network.size() as u32)).0;
        if tcp_header.sequence_number() != expected_packet {
            cx_warn!(
                target: TAG,
                self.id,
                "Ignoring packet {} (acking {}); expecting {}; flags={}",
                tcp_header.sequence_number(),
                tcp_header.acknowledgement_number(),
                expected_packet,
                tcp_header.flags()
            );
            return;
        }

        self.tcb.client_window = tcp_header.window();
        self.tcb.their_acknowledgement_number = tcp_header.acknowledgement_number();

        cx_debug!(
            target: TAG,
            self.id,
            "Receiving expected packet {} (flags={})",
            tcp_header.sequence_number(),
            tcp_header.flags()
        );

        if tcp_header.is_rst() {
            self.close();
            return;
        }

        if tcp_header.is_ack() {
            cx_debug!(
                target: TAG,
                self.id,
                "Client acked {}",
                tcp_header.acknowledgement_number()
            );

            self.handle_ack(client_channel, ip_packet);
        }

        if tcp_header.is_fin() {
            self.handle_fin(client_channel);
        }

        if let Some(fin_sequence_number) = self.tcb.fin_sequence_number
            && tcp_header.acknowledgement_number() == fin_sequence_number + 1 {
                cx_debug!(target: TAG, self.id, "Received ACK of FIN");
                self.handle_fin_ack();
            }
    }

    fn handle_first_packet(
        &mut self,
        client_channel: &mut ClientChannel,
        ip_packet: &IpPacket,
    ) {
        cx_debug!(target: TAG, self.id, "handle_first_packet()");
        let tcp_header = Self::tcp_header_of_packet(ip_packet);
        if tcp_header.is_syn() {
            let their_sequence_number = tcp_header.sequence_number();
            self.tcb.acknowledgement_number = Wrapping(their_sequence_number) + Wrapping(1);
            self.tcb.syn_sequence_number = their_sequence_number;

            self.tcb.sequence_number = Wrapping(random::<u32>());
            cx_debug!(
                target: TAG,
                self.id,
                "Initialized seq={}; ack={}",
                self.tcb.sequence_number,
                self.tcb.acknowledgement_number
            );
            self.tcb.client_window = tcp_header.window();
            self.tcb.state = TcpState::SynSent;
            cx_debug!(target: TAG, self.id, "State = {:?}", self.tcb.state);
        } else {
            cx_warn!(
                target: TAG,
                self.id,
                "Unexpected first packet {}; acking {}; flags={}",
                tcp_header.sequence_number(),
                tcp_header.acknowledgement_number(),
                tcp_header.flags()
            );
            self.tcb.sequence_number = Wrapping(tcp_header.acknowledgement_number());
            self.reply_empty_packet_to_client(client_channel, tcp_header::FLAG_RST);
            self.close();
        }
    }

    fn handle_duplicate_syn(
        &mut self,
        client_channel: &mut ClientChannel,
        ip_packet: &IpPacket,
    ) {
        let tcp_header = Self::tcp_header_of_packet(ip_packet);
        let their_sequence_number = tcp_header.sequence_number();
        if self.tcb.state == TcpState::SynSent {
            self.tcb.syn_sequence_number = their_sequence_number;
            self.tcb.acknowledgement_number = Wrapping(their_sequence_number) + Wrapping(1);
        } else if their_sequence_number != self.tcb.syn_sequence_number {
            self.reply_empty_packet_to_client(client_channel, tcp_header::FLAG_RST);
            self.close();
        }
    }

    fn handle_fin(&mut self, client_channel: &mut ClientChannel) {
        cx_debug!(
            target: TAG,
            self.id,
            "Received a FIN from the client {}",
            self.tcb.numbers()
        );

        self.tcb.fin_received = true;
        if self.client_to_network.is_empty() {
            cx_debug!(
                target: TAG,
                self.id,
                "No pending data, process the FIN immediately"
            );
            self.do_handle_fin(client_channel);
        }
    }

    fn do_handle_fin(&mut self, client_channel: &mut ClientChannel) {
        self.tcb.acknowledgement_number += Wrapping(1); // received FIN counts for 1 byte

        if self.tcb.state == TcpState::Established {
            self.reply_empty_packet_to_client(
                client_channel,
                tcp_header::FLAG_FIN | tcp_header::FLAG_ACK,
            );
            self.tcb.fin_sequence_number = Some(self.tcb.sequence_number.0);
            self.tcb.sequence_number += Wrapping(1);
            self.tcb.state = TcpState::LastAck;
            cx_debug!(target: TAG, self.id, "State = {:?}", self.tcb.state);
        } else if self.tcb.state == TcpState::FinWait1 {
            self.reply_empty_packet_to_client(client_channel, tcp_header::FLAG_ACK);
            self.tcb.state = TcpState::Closing;
            cx_debug!(target: TAG, self.id, "State = {:?}", self.tcb.state);
        } else if self.tcb.state == TcpState::FinWait2 {
            self.reply_empty_packet_to_client(client_channel, tcp_header::FLAG_ACK);
            self.close();
        } else {
            cx_warn!(
                target: TAG,
                self.id,
                "Received FIN was state was {:?}",
                self.tcb.state
            );
        }
    }

    fn handle_fin_ack(&mut self) {
        if self.tcb.state == TcpState::LastAck || self.tcb.state == TcpState::Closing {
            self.close();
        } else if self.tcb.state == TcpState::FinWait1 {
            self.tcb.state = TcpState::FinWait2;
            cx_debug!(target: TAG, self.id, "State = {:?}", self.tcb.state);
        } else if self.tcb.state != TcpState::FinWait2 {
            cx_warn!(
                target: TAG,
                self.id,
                "Received FIN ACK while state was {:?}",
                self.tcb.state
            );
        }
    }

    fn handle_ack(
        &mut self,
        _client_channel: &mut ClientChannel,
        ip_packet: &IpPacket,
    ) {
        cx_debug!(target: TAG, self.id, "handle_ack()");
        if self.tcb.state == TcpState::SynReceived {
            self.tcb.state = TcpState::Established;
            cx_debug!(target: TAG, self.id, "State = {:?}", self.tcb.state);
            return;
        }

        if log_enabled!(target: TAG, Level::Trace) {
            cx_trace!(
                target: TAG,
                self.id,
                "{}",
                binary::build_packet_string(ip_packet.raw())
            );
        }

        let payload = match ip_packet.payload() {
            Some(p) => p,
            None => return,
        };
        if payload.is_empty() {
            return;
        }

        if self.client_to_network.remaining() < payload.len() {
            cx_warn!(target: TAG, self.id, "Not enough space, dropping packet");
            return;
        }

        self.client_to_network.read_from(payload);
    }

    fn create_empty_response_packet<'a>(
        id: &ConnectionId,
        packetizer: &'a mut Packetizer,
        tcb: &Tcb,
        flags: u16,
    ) -> IpPacket<'a> {
        Self::update_headers(packetizer, tcb, flags);
        cx_debug!(
            target: TAG,
            id,
            "Forging empty response (flags={}) {}",
            flags,
            tcb.numbers()
        );
        if (flags & tcp_header::FLAG_ACK) != 0 {
            cx_debug!(target: TAG, id, "Acking {}", tcb.numbers());
        }
        let ip_packet = packetizer.packetize_empty_payload();
        if log_enabled!(target: TAG, Level::Trace) {
            cx_trace!(
                target: TAG,
                id,
                "{}",
                binary::build_packet_string(ip_packet.raw())
            );
        }
        ip_packet
    }

    fn may_read(&self) -> bool {
        if !self.tcb.state.is_connected() || self.tcb.state.is_closed() {
            return false;
        }
        if self.packet_for_client_length.is_some() {
            return false;
        }
        self.tcb.remaining_client_window() > 0
    }

    fn may_write(&self) -> bool {
        !self.client_to_network.is_empty()
    }
}

impl Connection for TcpConnection {
    fn id(&self) -> &ConnectionId {
        &self.id
    }

    fn send_to_network(
        &mut self,
        client_channel: &mut ClientChannel,
        ip_packet: &IpPacket,
    ) {
        self.handle_packet(client_channel, ip_packet);
    }

    fn close(&mut self) {
        cx_info!(target: TAG, self.id, "Close");
        self.closed = true;
    }

    fn is_expired(&self) -> bool {
        false
    }

    fn is_closed(&self) -> bool {
        self.closed
    }

    fn poll(&mut self) -> io::Result<()> {
        self.poll_self()
    }
}

impl PacketSource for TcpConnection {
    fn get(&mut self) -> Option<IpPacket<'_>> {
        if let Some(len) = self.packet_for_client_length {
            Some(self.network_to_client.inflate(len))
        } else {
            None
        }
    }

    fn next(&mut self) {
        let len = match self.packet_for_client_length {
            Some(l) => l,
            None => {
                error!(target: TAG, "next() called with no pending packet");
                return;
            }
        };
        cx_debug!(
            target: TAG,
            self.id,
            "Deferred packet ({} bytes) sent to client {}",
            len,
            self.tcb.numbers()
        );
        self.tcb.sequence_number += Wrapping(u32::from(len));
        self.packet_for_client_length = None;
    }
}
