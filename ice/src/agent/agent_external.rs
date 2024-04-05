use std::{collections::VecDeque, io::Result, net::SocketAddr, sync::Arc, thread, time::Duration};
use tokio::sync::Mutex;

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

#[inline]
fn serialize_socket_addr(addr: SocketAddr) -> Vec<u8> {
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