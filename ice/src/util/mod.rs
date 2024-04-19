#[cfg(test)]
mod util_test;

use std::collections::HashSet;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, UdpSocket};
use std::ops::Add;
use std::sync::Arc;

use log::{debug, info};
use stun::agent::*;
use stun::attributes::*;
use stun::integrity::*;
use stun::message::*;
use stun::textattrs::*;
use stun::xoraddr::*;
use tokio::time::Duration;
use util::vnet::net::*;
use util::Conn;

use crate::agent::agent_config::{InterfaceFilterFn, IpFilterFn};
use crate::agent::agent_external::{parse_recv_info, parse_send_info, serialize_send_info, SendInfo};
use crate::error::*;
use crate::network_type::*;

pub fn create_addr(_network: NetworkType, ip: IpAddr, port: u16) -> SocketAddr {
    /*if network.is_tcp(){
        return &net.TCPAddr{IP: ip, Port: port}
    default:
        return &net.UDPAddr{IP: ip, Port: port}
    }*/
    SocketAddr::new(ip, port)
}

pub fn assert_inbound_username(m: &Message, expected_username: &str) -> Result<()> {
    let mut username = Username::new(ATTR_USERNAME, String::new());
    username.get_from(m)?;

    if username.to_string() != expected_username {
        return Err(Error::Other(format!(
            "{:?} expected({}) actual({})",
            Error::ErrMismatchUsername,
            expected_username,
            username,
        )));
    }

    Ok(())
}

pub fn assert_inbound_message_integrity(m: &mut Message, key: &[u8]) -> Result<()> {
    let message_integrity_attr = MessageIntegrity(key.to_vec());
    Ok(message_integrity_attr.check(m)?)
}

/// Initiates a stun requests to `server_addr` using conn, reads the response and returns the
/// `XORMappedAddress` returned by the stun server.
/// Adapted from stun v0.2.
pub async fn get_xormapped_addr(
    conn: &Arc<dyn Conn + Send + Sync>,
    server_addr: SocketAddr,
    deadline: Duration,
) -> Result<(XorMappedAddress, SocketAddr)> {
    let resp = stun_request(conn, server_addr, deadline).await?;
    // info!("Stun request successful...");
    let mut addr = XorMappedAddress::default();
    addr.get_from(&resp.0)?;
    Ok((addr, resp.1))
}

const MAX_MESSAGE_SIZE: usize = 1280;

// Idea: Replace the binding of the socket to the correct address with a
// binding to a localhost socket and insert the correct address mapping
// into any type of easy to retrieve storage. Connect to a localhost
// address where the other application is listening on and send a UDP in UDP
// packet to the socket which then know where to forward this informatiosn to.
// The external application needs to store the to and from mapping (very)
// similar to the actual NAT we are trying to navigate and allows sending
// the packet back to the socket opened by ice. To allow for an easy 
// management and differentiation bind to different ports. ~10000 addresses
// should be enough for anything to work with
pub async fn stun_request(
    conn: &Arc<dyn Conn + Send + Sync>,
    server_addr: SocketAddr,
    deadline: Duration,
) -> Result<(Message, SocketAddr)> {
    // Modifying the 'server' addr to be contained in the packet
    // The packet is also relayed via quicheperf to obtain control
    // over the socket
    let relayed_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(127, 0, 0, 1)), 12345);
    let send_info = SendInfo {
        from: conn.local_addr().unwrap(),
        to: server_addr,
    };
    // info!("STUN request send info: {:?}", send_info);
    let mut send_info_raw = serialize_send_info(send_info).unwrap();

    let mut request = Message::new();
    request.build(&[Box::new(BINDING_REQUEST), Box::new(TransactionId::new())])?;
    send_info_raw.append(&mut request.raw);
    
    conn.send_to(&send_info_raw, relayed_addr).await?;
    
    let mut bs = vec![0_u8; MAX_MESSAGE_SIZE];
    let (n, _) = if deadline > Duration::from_secs(0) {
        // TODO: Increase the timeout duration since we have the ICE indirection
        match tokio::time::timeout(deadline.add(Duration::from_millis(200)), conn.recv_from(&mut bs)).await {
            Ok(result) => match result {
                Ok((n, addr)) => (n, addr),
                Err(err) => return Err(Error::Other(err.to_string())),
            },
            Err(err) => return Err(Error::Other(err.to_string())),
        }
    } else {
        conn.recv_from(&mut bs).await?
    };

    // Check if we received a relayed packet or not
    let mut res = Message::new();
    let p_type = bs[0];
    let mut local_addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), 0);
    match p_type {
        0xCC => {
            let len = bs[1];
            let recv_info = parse_send_info(&bs[2..], len as usize).unwrap();
            // TODO: Check if we need to do something with the from information or not
            info!("Received relayed STUN response from {}->{}", recv_info.from, recv_info.to);
            local_addr = recv_info.to;
            res.raw = bs[(2 + len as usize)..n].to_vec();
            res.decode()?;
        },
        _ => {
            res.raw = bs[..n].to_vec();
            res.decode()?;
        }
    }
    Ok((res, local_addr))
}

pub async fn local_interfaces(
    vnet: &Arc<Net>,
    interface_filter: &Option<InterfaceFilterFn>,
    ip_filter: &Option<IpFilterFn>,
    network_types: &[NetworkType],
) -> HashSet<IpAddr> {
    let mut ips = HashSet::new();
    let interfaces = vnet.get_interfaces().await;

    let (mut ipv4requested, mut ipv6requested) = (false, false);
    for typ in network_types {
        if typ.is_ipv4() {
            ipv4requested = true;
        }
        if typ.is_ipv6() {
            ipv6requested = true;
        }
    }

    for iface in interfaces {
        if let Some(filter) = interface_filter {
            if !filter(iface.name()) {
                continue;
            }
        }

        for ipnet in iface.addrs() {
            let ipaddr = ipnet.addr();

            if !ipaddr.is_loopback()
                && ((ipv4requested && ipaddr.is_ipv4()) || (ipv6requested && ipaddr.is_ipv6()))
                && ip_filter
                    .as_ref()
                    .map(|filter| filter(ipaddr))
                    .unwrap_or(true)
            {
                ips.insert(ipaddr);
            }
        }
    }

    ips
}

pub async fn listen_udp_in_port_range(
    vnet: &Arc<Net>,
    port_max: u16,
    port_min: u16,
    laddr: SocketAddr,
) -> Result<Arc<dyn Conn + Send + Sync>> {
    if laddr.port() != 0 || (port_min == 0 && port_max == 0) {
        return Ok(vnet.bind(laddr).await?);
    }
    let i = if port_min == 0 { 1 } else { port_min };
    let j = if port_max == 0 { 0xFFFF } else { port_max };
    if i > j {
        return Err(Error::ErrPort);
    }

    let port_start = rand::random::<u16>() % (j - i + 1) + i;
    let mut port_current = port_start;
    loop {
        let laddr = SocketAddr::new(laddr.ip(), port_current);
        match vnet.bind(laddr).await {
            Ok(c) => return Ok(c),
            Err(err) => log::debug!("failed to listen {}: {}", laddr, err),
        };

        port_current += 1;
        if port_current > j {
            port_current = i;
        }
        if port_current == port_start {
            break;
        }
    }

    Err(Error::ErrPort)
}
