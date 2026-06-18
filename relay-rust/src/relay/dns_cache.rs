//! A simple DNS forwarder with an in-memory cache.
//! Binds to 10.0.2.2:53 (the Android emulator's gateway) to intercept DNS
//! queries from the device, caches responses, and forwards uncached queries
//! to an upstream DNS server (default 8.8.8.8:53).

#![allow(dead_code)]

use log::*;
use std::collections::HashMap;
use std::net::{Ipv4Addr, SocketAddr};
use std::sync::Arc;
use std::time::Instant;
use tokio::sync::Mutex;
use tokio::time::Duration;

const TAG: &str = "DnsCache";

const CACHE_TTL_SECONDS: u64 = 60;


/// DNS cache entry with expiry.
#[derive(Clone)]
struct CacheEntry {
    response: Vec<u8>,
    expires_at: Instant,
}

/// Shared DNS cache state.
pub struct DnsCache {
    cache: Arc<Mutex<HashMap<String, CacheEntry>>>,
    upstream: SocketAddr,
    bind_addr: SocketAddr,
}

impl DnsCache {
    /// Create a new DNS cache forwarder on the given bind address.
    pub fn new(bind_addr: SocketAddr) -> Self {
        Self {
            cache: Arc::new(Mutex::new(HashMap::new())),
            upstream: SocketAddr::new(Ipv4Addr::new(8, 8, 8, 8).into(), 53),
            bind_addr,
        }
    }

    /// Run the DNS forwarder: listen on the bind address and handle queries.
    pub async fn run(&self) {
        let sock = match tokio::net::UdpSocket::bind(self.bind_addr).await {
            Ok(s) => s,
            Err(e) => {
                error!(target: TAG, "Cannot bind DNS cache to {}: {}", self.bind_addr, e);
                return;
            }
        };
        info!(target: TAG, "DNS cache listening on {}", self.bind_addr);

        // Connect to upstream for sending queries
        let upstream_sock = match tokio::net::UdpSocket::bind("0.0.0.0:0").await {
            Ok(s) => s,
            Err(e) => {
                error!(target: TAG, "Cannot bind upstream socket: {}", e);
                return;
            }
        };

        let mut buf = [0u8; 512]; // Standard DNS max size
        let cache = self.cache.clone();
        let upstream = self.upstream;

        loop {
            let (len, client_addr) = match sock.recv_from(&mut buf).await {
                Ok(v) => v,
                Err(e) => {
                    error!(target: TAG, "DNS recv error: {}", e);
                    continue;
                }
            };

            let query = buf[..len].to_vec();
            let query_name = extract_dns_name(&query);

            let cached = if let Some(ref name) = query_name {
                let cache = cache.lock().await;
                cache.get(name).cloned()
            } else {
                None
            };

            if let Some(entry) = cached
                && entry.expires_at > Instant::now() {
                    trace!(target: TAG, "DNS cache hit: {:?}", query_name);
                    let _ = sock.send_to(&entry.response, client_addr).await;
                    continue;
                }

            // Forward to upstream DNS
            trace!(target: TAG, "DNS cache miss, forwarding: {:?}", query_name);
            if let Err(e) = upstream_sock.send_to(&query, upstream).await {
                error!(target: TAG, "Cannot forward DNS query: {}", e);
                continue;
            }

            let mut resp_buf = [0u8; 512];
            let resp_len = match upstream_sock.recv(&mut resp_buf).await {
                Ok(n) => n,
                Err(e) => {
                    error!(target: TAG, "Cannot receive DNS response: {}", e);
                    continue;
                }
            };

            let response = resp_buf[..resp_len].to_vec();

            // Cache the response
            if let Some(ref name) = query_name {
                let mut cache = cache.lock().await;
                cache.insert(
                    name.clone(),
                    CacheEntry {
                        response: response.clone(),
                        expires_at: Instant::now() + Duration::from_secs(CACHE_TTL_SECONDS),
                    },
                );
            }

            // Send response back to client
            let _ = sock.send_to(&response, client_addr).await;
        }
    }
}

/// Extract the first DNS name (QNAME) from a DNS query.
/// Returns None if the packet is too short or malformed.
fn extract_dns_name(packet: &[u8]) -> Option<String> {
    if packet.len() < 12 {
        return None;
    }
    // DNS header is 12 bytes, query starts at offset 12
    let mut offset = 12;
    let mut labels = Vec::new();
    loop {
        if offset >= packet.len() {
            return None;
        }
        let len = packet[offset] as usize;
        if len == 0 {
            break; // End of labels
        }
        // Check for compression (top 2 bits set) — not expected in queries
        if len & 0xC0 == 0xC0 {
            break;
        }
        if offset + 1 + len > packet.len() {
            return None;
        }
        let label = &packet[offset + 1..offset + 1 + len];
        labels.push(String::from_utf8_lossy(label).to_lowercase());
        offset += 1 + len;
    }
    if labels.is_empty() {
        None
    } else {
        Some(labels.join("."))
    }
}
