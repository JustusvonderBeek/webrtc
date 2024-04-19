use std::{collections::VecDeque, io::{self, Result}, net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr}, sync::Arc};
use tokio::sync::Mutex;
use log::error;

pub const MAX_STUN_DATA: usize = 1500;
pub const SEND_INFO_PACKET_TYPE : u8 = 0xAA;

#[derive(Debug)]
pub enum IceCommands {
    StunRequest {
        data: String,
        from: SocketAddr,
        to: SocketAddr,
    },
    StunResponse {
        data: [u8; MAX_STUN_DATA],
        len: usize,
        from: SocketAddr,
    },
    OpenSocket {
        addr: SocketAddr
    },
}

#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct SendInfo {
    // Size as u8 should be enough: 
    // Max. SocketAddr size == 16:IPv6 + 2:Port = 18 ; 2 * 18 = 36
    pub from: SocketAddr,
    pub to: SocketAddr,
}

pub(crate) struct AgentExternal {
    egress_msg: VecDeque<String>,
    ingress_mgs: VecDeque<String>,
}

pub fn serialize_socket_addr(addr: SocketAddr) -> Vec<u8> {
    let mut out : Vec<u8> = Vec::new();
    let mut ip = match addr.ip() {
        std::net::IpAddr::V4(ip) => {
            let octets = ip.octets();
            let mut ip_vec : Vec<u8> = Vec::new();
            ip_vec.extend_from_slice(&octets);
            ip_vec
        },
        std::net::IpAddr::V6(ip) => {
            let octets = ip.octets();
            let mut ip_vec : Vec<u8> = Vec::new();
            ip_vec.extend_from_slice(&octets);
            ip_vec
        },
    };
    out.append(&mut ip);
    out.extend_from_slice(&addr.port().to_be_bytes());
    out
}

pub fn serialize_send_info(send_info: SendInfo) -> Result<Vec<u8>> {
    let mut serialized = Vec::new();
    serialized.append(&mut serialize_socket_addr(send_info.from));
    serialized.append(&mut serialize_socket_addr(send_info.to));
    let size = serialized.len() as u8;
    let size_serialized = size.to_be_bytes();
    serialized.insert(0, size_serialized[0]);

    // To differentiate easily between the two packet types, include
    // some magic number in this type of packet first
    serialized.insert(0, SEND_INFO_PACKET_TYPE);

    Ok(serialized)
}

pub fn parse_recv_info(buf: &[u8], len: usize) -> Result<SocketAddr> {
    if len < 6 {
        return Err(io::Error::other(crate::Error::ErrAddressParseFailed));
    }
    let addr = match len {
        6 => {
            let raw_ip : [u8; 4] = buf[0..4].try_into().unwrap();
            let raw_port : [u8; 2] = buf[4..6].try_into().unwrap();
            let from_ip = Ipv4Addr::from(raw_ip);
            let from_port = u16::from_be_bytes(raw_port);
            let s_addr = SocketAddr::new(IpAddr::V4(from_ip), from_port);
            s_addr
        },
        18 => {
            let raw_ip : [u8; 16] = buf[0..16].try_into().unwrap();
            let raw_port : [u8; 2] = buf[16..18].try_into().unwrap();
            let from_ip = Ipv6Addr::from(raw_ip);
            let from_port = u16::from_be_bytes(raw_port);
            let s_addr = SocketAddr::new(IpAddr::V6(from_ip), from_port);
            s_addr
        },
        _ => {
            return Err(io::Error::other(crate::Error::ErrAddressParseFailed));
        }
    };
    Ok(addr)
}

pub fn parse_send_info(buf: &[u8], len: usize) -> Result<SendInfo> {
    let send_info = match len  {
        // 2 * IPv4 = 2 * (4 + 2) = 12
        12 => {
            // IPv4 to IPv4
            let raw_ip : [u8; 4] = buf[0..4].try_into().unwrap();
            let raw_port : [u8; 2] = buf[4..6].try_into().unwrap();
            let from_ip = Ipv4Addr::from(raw_ip);
            let from_port = u16::from_be_bytes(raw_port);
            let s_addr = SocketAddr::new(IpAddr::V4(from_ip), from_port);
            
            let raw_d_ip : [u8; 4] = buf[6..10].try_into().unwrap();
            let raw_d_port : [u8; 2] = buf[10..12].try_into().unwrap();
            let to_ip = Ipv4Addr::from(raw_d_ip);
            let to_port = u16::from_be_bytes(raw_d_port);
            let d_addr = SocketAddr::new(IpAddr::V4(to_ip), to_port);
            let s_info = SendInfo {
                from: s_addr,
                to: d_addr,
            };
            s_info
        },
        // 2 * IPv6 = 2 * (16 + 2) = 36
        36 => {
            let raw_ip : [u8; 16] = buf[0..16].try_into().unwrap();
            let raw_port : [u8; 2] = buf[16..18].try_into().unwrap();
            let from_ip = Ipv6Addr::from(raw_ip);
            let from_port = u16::from_be_bytes(raw_port);
            let s_addr = SocketAddr::new(IpAddr::V6(from_ip), from_port);
            
            let raw_d_ip : [u8; 16] = buf[18..34].try_into().unwrap();
            let raw_d_port : [u8; 2] = buf[34..36].try_into().unwrap();
            let to_ip = Ipv6Addr::from(raw_d_ip);
            let to_port = u16::from_be_bytes(raw_d_port);
            let d_addr = SocketAddr::new(IpAddr::V6(to_ip), to_port);
            let s_info = SendInfo{
                from: s_addr,
                to: d_addr,
            };
            s_info
        },
        _ => {
            error!("Given send info size {} cannot be parsed", len);
            return Err(io::Error::other(crate::Error::ErrAddressParseFailed));
        }
    };
    Ok(send_info)
}

impl AgentExternal {
    pub(crate) fn new() -> AgentExternal {
        return AgentExternal {
            egress_msg: VecDeque::new(),
            ingress_mgs: VecDeque::new(),
        };
    }
    pub(crate) fn send_message(&mut self, message: String) {
        self.egress_msg.push_back(message);
    }
    pub(crate) fn get_message(&mut self) -> Option<String> {
        self.ingress_mgs.pop_front()
    }
}

// TODO: Change this to mio polling
// pub(crate) fn start_external_listener(external: Arc<Mutex<AgentExternal>>, rx: channel::Receiver<String>) -> Result<()> {
//     // tokio::spawn(async move {
//     //     let rx = rx.borrow();
//     //     loop {
//     //         match rx.try_recv() {
//     //             Ok(s) => {
//     //                 // TODO: Parse
//     //                 external.lock().await.ingress_mgs.push_back(s);
//     //             },
//     //             Err(e) => {
//     //                 thread::sleep(Duration::from_millis(100));
//     //             }
//     //         }
//     //     }
//     // });
//     Ok(())
// }

// pub(crate) fn start_external_send(external: Arc<Mutex<AgentExternal>>, tx: channel::Sender<String>) -> Result<()> {
//     tokio::spawn(async move{
//         loop {
//             let mut agent = external.lock().await;
//             let next = agent.egress_msg.pop_front();
//             if next.is_none() {
//                 thread::sleep(Duration::from_millis(100));
//             } else {
//                 tx.send(next.unwrap()).unwrap();
//             }
//         }
//     });
//     Ok(())
// }

pub(crate) async fn send_external(external: Arc<Mutex<AgentExternal>>, msg: String) -> Result<()> {
    let mut agent = external.lock().await;
    agent.send_message(msg);
    Ok(())
}