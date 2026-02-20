use crate::net::NETWORK;
use crate::serial_println;
use alloc::vec;
use alloc::vec::Vec;
use smoltcp::socket::udp::{PacketBuffer, PacketMetadata, Socket as UdpSocket};
use smoltcp::time::Instant;
use smoltcp::wire::{IpAddress, IpEndpoint, Ipv4Address};

/// QEMU SLIRP default DNS server
const DNS_SERVER: Ipv4Address = Ipv4Address::new(10, 0, 2, 3);
const DNS_PORT: u16 = 53;
const LOCAL_PORT: u16 = 41234;

/// Resolve a domain name to an IPv4 address using a minimal DNS stub resolver.
/// Constructs a raw DNS query packet, sends it over UDP, polls for a response,
/// and parses the first A record from the answer section.
pub fn resolve(domain: &str) -> Option<[u8; 4]> {
    let query = build_dns_query(domain);

    let mut net_guard = NETWORK.lock();
    let net = net_guard.as_mut()?;

    // Create UDP socket with small buffers
    let rx_buffer = PacketBuffer::new(vec![PacketMetadata::EMPTY; 4], vec![0u8; 1024]);
    let tx_buffer = PacketBuffer::new(vec![PacketMetadata::EMPTY; 4], vec![0u8; 1024]);
    let mut socket = UdpSocket::new(rx_buffer, tx_buffer);
    socket.bind(LOCAL_PORT).ok()?;

    let handle = net.sockets.add(socket);

    // Send the DNS query
    {
        let socket = net.sockets.get_mut::<UdpSocket>(handle);
        let endpoint = IpEndpoint::new(IpAddress::Ipv4(DNS_SERVER), DNS_PORT);
        socket.send_slice(&query, endpoint).ok()?;
    }

    // Poll to push the packet out and wait for a response
    let mut result: Option<[u8; 4]> = None;
    for tick in 0..200 {
        net.iface.poll(
            Instant::from_millis((tick * 10) as i64),
            &mut net.device,
            &mut net.sockets,
        );

        let socket = net.sockets.get_mut::<UdpSocket>(handle);
        if socket.can_recv() {
            let mut buf = vec![0u8; 512];
            if let Ok((size, _)) = socket.recv_slice(&mut buf) {
                if size > 12 {
                    result = parse_dns_response(&buf[..size]);
                    break;
                }
            }
        }
    }

    net.sockets.remove(handle);

    if let Some(ip) = result {
        serial_println!(
            "[DNS] Resolved {} -> {}.{}.{}.{}",
            domain,
            ip[0],
            ip[1],
            ip[2],
            ip[3]
        );
    } else {
        serial_println!("[DNS] Failed to resolve {}", domain);
    }

    result
}

/// Build a minimal DNS A-record query packet for the given domain.
fn build_dns_query(domain: &str) -> Vec<u8> {
    let mut pkt = Vec::with_capacity(64);

    // Header (12 bytes)
    // Transaction ID
    pkt.extend_from_slice(&[0xAB, 0xCD]);
    // Flags: standard query, recursion desired
    pkt.extend_from_slice(&[0x01, 0x00]);
    // QDCOUNT = 1
    pkt.extend_from_slice(&[0x00, 0x01]);
    // ANCOUNT = 0
    pkt.extend_from_slice(&[0x00, 0x00]);
    // NSCOUNT = 0
    pkt.extend_from_slice(&[0x00, 0x00]);
    // ARCOUNT = 0
    pkt.extend_from_slice(&[0x00, 0x00]);

    // Question section: encode domain as DNS labels
    for label in domain.split('.') {
        pkt.push(label.len() as u8);
        pkt.extend_from_slice(label.as_bytes());
    }
    pkt.push(0x00); // Root label terminator

    // QTYPE = A (1)
    pkt.extend_from_slice(&[0x00, 0x01]);
    // QCLASS = IN (1)
    pkt.extend_from_slice(&[0x00, 0x01]);

    pkt
}

/// Parse a DNS response and extract the first A record's IPv4 address.
fn parse_dns_response(data: &[u8]) -> Option<[u8; 4]> {
    if data.len() < 12 {
        return None;
    }

    let ancount = u16::from_be_bytes([data[6], data[7]]) as usize;
    if ancount == 0 {
        return None;
    }

    // Skip the header (12 bytes) and the question section
    let mut offset = 12;

    // Skip question: walk labels until null terminator
    while offset < data.len() && data[offset] != 0 {
        let len = data[offset] as usize;
        offset += 1 + len;
    }
    offset += 1; // null terminator
    offset += 4; // QTYPE (2) + QCLASS (2)

    // Parse answer records
    for _ in 0..ancount {
        if offset + 12 > data.len() {
            return None;
        }

        // Skip name (handle compression pointers)
        if data[offset] & 0xC0 == 0xC0 {
            offset += 2; // Compressed pointer
        } else {
            while offset < data.len() && data[offset] != 0 {
                let len = data[offset] as usize;
                offset += 1 + len;
            }
            offset += 1;
        }

        if offset + 10 > data.len() {
            return None;
        }

        let rtype = u16::from_be_bytes([data[offset], data[offset + 1]]);
        let rdlength = u16::from_be_bytes([data[offset + 8], data[offset + 9]]) as usize;
        offset += 10;

        if rtype == 1 && rdlength == 4 && offset + 4 <= data.len() {
            return Some([
                data[offset],
                data[offset + 1],
                data[offset + 2],
                data[offset + 3],
            ]);
        }

        offset += rdlength;
    }

    None
}
