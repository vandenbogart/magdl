mod magnet;
mod peer_codec;
mod peer_message;
mod tracker_stream;
use byteorder::{BigEndian, ByteOrder};
use bytes::Bytes;
use futures::{SinkExt, StreamExt};
use peer_codec::{Handshake, PeerCodec, PeerFrame, BITTORRENT_PROTOCOL};
use peer_message::{PeerMessage, PeerMessageType};
use std::{collections::HashMap, net::SocketAddr, sync::Arc, time::Duration};
use tokio_util::codec::Framed;

use magnet::Magnet;
use rand::Rng;
use tokio::{
    net::TcpStream,
    sync::{
        mpsc::{self, UnboundedReceiver, UnboundedSender},
        Mutex, RwLock,
    },
};
use tracker_stream::Trackers;

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let link = "magnet:?xt=urn:btih:73103935E5CA2B132DA9C5B716A012CEFC67E6BA&dn=Succession.S03E06.1080p.WEB.H264-CAKES&tr=http%3A%2F%2Ftracker.trackerfix.com%3A80%2Fannounce&tr=udp%3A%2F%2F9.rarbg.me%3A2800%2Fannounce&tr=udp%3A%2F%2F9.rarbg.to%3A2950%2Fannounce&tr=udp%3A%2F%2Ftracker.thinelephant.org%3A12740%2Fannounce&tr=udp%3A%2F%2Ftracker.fatkhoala.org%3A13720%2Fannounce&tr=udp%3A%2F%2Ftracker.opentrackr.org%3A1337%2Fannounce&tr=http%3A%2F%2Ftracker.openbittorrent.com%3A80%2Fannounce&tr=udp%3A%2F%2Fopentracker.i2p.rocks%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.internetwarriors.net%3A1337%2Fannounce&tr=udp%3A%2F%2Ftracker.leechers-paradise.org%3A6969%2Fannounce&tr=udp%3A%2F%2Fcoppersurfer.tk%3A6969%2Fannounce&tr=udp%3A%2F%2Ftracker.zer0day.to%3A1337%2Fannounce";
    let magnet = Magnet::from_link_string(link);

    let state = Arc::new(RwLock::new(Shared::new(magnet.info_hash.to_vec().into())));

    let trackers = Trackers::new(&magnet.tracker_urls).await;

    {
        let state_lock = state.read().await;
        let peers = trackers
            .announce(state_lock.peer_id.clone(), magnet.info_hash.to_vec().into())
            .await;
        for addr in peers.into_iter() {
            let state = Arc::clone(&state);
            tokio::spawn(async move {
                if let Err(e) = peer_process(state, addr).await {
                    println!("{:#}", e);
                }
            });
        }
    }

    loop {
        tokio::task::yield_now().await;
        let state = state.read().await;
        let peers = state.peer_state.keys().len();
        let unchoked_peers =
            state
                .peer_state
                .values()
                .fold(0, |a, p| if !p.am_choked { a + 1 } else { a });
        println!("Unchoked Peers: {}/{}", unchoked_peers, peers);
    }
}

async fn peer_process(state: Arc<RwLock<Shared>>, addr: SocketAddr) -> anyhow::Result<()> {
    let conn_future = TcpStream::connect(addr);
    let conn = tokio::time::timeout(Duration::from_secs(5), conn_future).await??;
    let mut framed = Framed::new(conn, PeerCodec::new());

    let info_hash = {
        let state = state.read().await;
        let handshake = Handshake {
            pstr: BITTORRENT_PROTOCOL.into(),
            info_hash: state.info_hash.clone(),
            peer_id: state.peer_id.clone(),
        };
        let hs_frame = PeerFrame::Handshake(handshake);
        framed.send(hs_frame).await?;
        state.info_hash.clone()
    };
    let process_peer_id = match framed.next().await {
        Some(Ok(PeerFrame::Handshake(hs))) => {
            if hs.info_hash != info_hash {
                anyhow::bail!("Bad info hash");
            }
            hs.peer_id
        }
        Some(Ok(_)) => {
            anyhow::bail!("No handshake received");
        }
        Some(Err(e)) => {
            anyhow::bail!(e);
        }
        None => {
            anyhow::bail!("Connection reset by peer");
        }
    };
    let mut peer = Peer::new(process_peer_id, state.clone(), framed).await?;

    while let Some(frame) = peer.framed.next().await {
        match frame {
            Ok(PeerFrame::Data(data)) => {
                let message = PeerMessage {
                    message_type: PeerMessageType::from(data.message_id),
                    payload: data.payload,
                };
                peer.handle_message(message).await;
            }
            Ok(_) => {
                peer.cleanup().await?;
                anyhow::bail!("Invalid message");
            }
            Err(e) => {
                peer.cleanup().await?;
                anyhow::bail!(e);
            }
        }
    }
    peer.cleanup().await?;
    Ok(())
}

struct PeerState {
    choked: bool,
    interested: bool,
    am_choked: bool,
    am_interested: bool,
    bitfield: Vec<bool>,
}
impl Default for PeerState {
    fn default() -> Self {
        Self {
            choked: true,
            interested: false,
            am_choked: true,
            am_interested: false,
            bitfield: Vec::new(),
        }
    }
}

pub struct Peer {
    shared: Arc<RwLock<Shared>>,
    process_peer_id: Bytes,
    framed: Framed<TcpStream, PeerCodec>,
    rx: UnboundedReceiver<PeerMessage>,
    addr: SocketAddr,
}
impl Peer {
    async fn new(
        process_peer_id: Bytes,
        shared: Arc<RwLock<Shared>>,
        framed: Framed<TcpStream, PeerCodec>,
    ) -> anyhow::Result<Self> {
        let addr = framed.get_ref().peer_addr()?;
        let (tx, rx) = mpsc::unbounded_channel::<PeerMessage>();

        {
            let mut state = shared.write().await;
            state.peer_channels.insert(addr, tx);
            state.peer_state.insert(addr, PeerState::default());
        }

        Ok(Self {
            shared,
            process_peer_id,
            framed,
            rx,
            addr,
        })
    }
    async fn cleanup(&mut self) -> anyhow::Result<()> {
        let addr = self.framed.get_ref().peer_addr()?;
        let mut state = self.shared.write().await;
        state.peer_channels.remove(&addr);
        state.peer_state.remove(&addr);
        Ok(())
    }
    async fn handle_message(&mut self, message: PeerMessage) {
        let mut shared = self.shared.write().await;
        let mut peer_state = shared.peer_state.get_mut(&self.addr).unwrap();
        match message.message_type {
            peer_message::PeerMessageType::Choke => peer_state.am_choked = true,
            peer_message::PeerMessageType::Unchoke => peer_state.am_choked = false,
            peer_message::PeerMessageType::Interested => peer_state.interested = true,
            peer_message::PeerMessageType::NotInterested => peer_state.interested = false,
            peer_message::PeerMessageType::Have => {
                peer_state.bitfield[BigEndian::read_u32(&message.payload) as usize] = true
            }
            peer_message::PeerMessageType::Bitfield => {
                peer_state.bitfield = Peer::process_bitfield(message)
            }
            peer_message::PeerMessageType::Request => todo!(),
            peer_message::PeerMessageType::Piece => todo!(),
            peer_message::PeerMessageType::Cancel => todo!(),
            peer_message::PeerMessageType::Port => todo!(),
        }
    }
    fn process_bitfield(message: PeerMessage) -> Vec<bool> {
        let mask = 0b10000000;
        let mut bits = Vec::new();
        message.payload.into_iter().for_each(|byte| {
            for i in 0..8 {
                let s_mask = mask >> i;
                let bit = (byte & s_mask) > 0;
                bits.push(bit);
            }
        });
        bits
    }
}

struct Shared {
    info_hash: Bytes,
    peer_id: Bytes,
    peer_channels: HashMap<SocketAddr, UnboundedSender<PeerMessage>>,
    peer_state: HashMap<SocketAddr, PeerState>,
}
impl Shared {
    fn new(info_hash: Bytes) -> Self {
        let mut peer_id = vec![0u8; 20];
        rand::thread_rng().fill(&mut peer_id[..]);
        let signature = "-WM0001-";
        peer_id[0..signature.len()].copy_from_slice(signature.as_bytes());

        Self {
            info_hash,
            peer_id: peer_id.into(),
            peer_channels: HashMap::new(),
            peer_state: HashMap::new(),
        }
    }
}
