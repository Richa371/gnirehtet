//! Trait for sources that can produce packets on demand (e.g., TcpConnection).
//! Used when a packet could not be sent immediately and must be deferred.

use super::ip_packet::IpPacket;

/// Source that may produce packets.
///
/// When a `TcpConnection` sends a packet to the `Client` while its buffers are full, then it
/// fails. To recover, once some space becomes available, the `Client` must pull the available
/// packets.
///
/// This trait provides the abstraction of a packet source from which it can pull packets.
///
/// It is implemented by `TcpConnection`.
pub trait PacketSource {
    fn get(&mut self) -> Option<IpPacket<'_>>;
    fn next(&mut self);
}
