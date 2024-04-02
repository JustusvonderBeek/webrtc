use std::{collections::VecDeque, io::Result, net::SocketAddr, sync::Arc, thread, time::Duration};
use tokio::sync::Mutex;
use mio_extras::channel;

pub const MAX_STUN_DATA: usize = 1500;

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

pub(crate) struct AgentExternal {
    egress_msg: VecDeque<String>,
    ingress_mgs: VecDeque<String>,
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
pub(crate) fn start_external_listener(external: Arc<Mutex<AgentExternal>>, rx: channel::Receiver<String>) -> Result<()> {
    // tokio::spawn(async move {
    //     let rx = rx.borrow();
    //     loop {
    //         match rx.try_recv() {
    //             Ok(s) => {
    //                 // TODO: Parse
    //                 external.lock().await.ingress_mgs.push_back(s);
    //             },
    //             Err(e) => {
    //                 thread::sleep(Duration::from_millis(100));
    //             }
    //         }
    //     }
    // });
    Ok(())
}

pub(crate) fn start_external_send(external: Arc<Mutex<AgentExternal>>, tx: channel::Sender<String>) -> Result<()> {
    tokio::spawn(async move{
        loop {
            let mut agent = external.lock().await;
            let next = agent.egress_msg.pop_front();
            if next.is_none() {
                thread::sleep(Duration::from_millis(100));
            } else {
                tx.send(next.unwrap()).unwrap();
            }
        }
    });
    Ok(())
}

pub(crate) async fn send_external(external: Arc<Mutex<AgentExternal>>, msg: String) -> Result<()> {
    let mut agent = external.lock().await;
    agent.send_message(msg);
    Ok(())
}