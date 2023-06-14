mod connection;
mod peer_connection;
mod tracker_stream;
//mod trackers;
mod magnet;
use std::{collections::HashMap, net::SocketAddr};

use byteorder::{BigEndian, ByteOrder};
use magnet::Magnet;
use peer_connection::PeerMessageType;
use rand::Rng;
use tokio::sync::mpsc;
use tracker_stream::Trackers;

use crate::peer_connection::{PeerConnection, PeerMessage};

#[derive(Debug)]
pub struct Peer {
    addr: SocketAddr,
    state: PeerState,
    sender: mpsc::Sender<PeerMessage>,
}

#[derive(Debug)]
pub struct PeerState {
    am_interested: bool,
    am_choked: bool,
    interested: bool,
    choked: bool,
    bitfield: Vec<bool>,
}
impl Default for PeerState {
    fn default() -> Self {
        Self {
            am_interested: false,
            am_choked: true,
            interested: false,
            choked: true,
            bitfield: Vec::new(),
        }
    }
}
impl PeerState {
    fn update(&mut self, message: PeerMessage) {
        match message.message_type {
            peer_connection::PeerMessageType::Choke => self.am_choked = true,
            peer_connection::PeerMessageType::Unchoke => self.am_choked = false,
            peer_connection::PeerMessageType::Interested => self.am_interested = true,
            peer_connection::PeerMessageType::NotInterested => self.am_interested = false,
            peer_connection::PeerMessageType::Have => self.process_have(message.payload),
            peer_connection::PeerMessageType::Bitfield => self.process_bitfield(message.payload),
            peer_connection::PeerMessageType::Request => todo!(),
            peer_connection::PeerMessageType::Piece => {
                dbg!(message);
            }
            peer_connection::PeerMessageType::Cancel => todo!(),
            peer_connection::PeerMessageType::Port => todo!(),
        }
    }
    fn process_bitfield(&mut self, bytes: Vec<u8>) {
        let mask = 0b10000000;
        let mut bits = Vec::new();
        bytes.into_iter().for_each(|byte| {
            for i in 0..8 {
                let s_mask = mask >> i;
                let bit = (byte & s_mask) > 0;
                bits.push(bit);
            }
        });
        self.bitfield = bits;
    }
    fn process_have(&mut self, bytes: Vec<u8>) {
        let index = BigEndian::read_u32(&bytes);
        self.bitfield[index as usize] = true;
    }
}

pub struct Magdl;
impl Magdl {
    pub async fn download(magnet_link: String) -> anyhow::Result<()> {
        let magnet = Magnet::from_link_string(magnet_link);

        let trackers = Trackers::new(&magnet.tracker_urls).await;

        let mut peer_id = [0u8; 20];
        rand::thread_rng().fill(&mut peer_id[..]);
        let signature = "-WM0001-";
        peer_id[0..signature.len()].copy_from_slice(signature.as_bytes());

        let peers = trackers.announce(peer_id, magnet.info_hash).await;

        let mut handles = Vec::new();
        let (tx, mut rx) = mpsc::channel::<PeerMessage>(32);
        let mut send_channels = HashMap::<SocketAddr, mpsc::Sender<PeerMessage>>::new();

        // Spawn peer communication tasks
        for peer in peers.into_iter() {
            let (o_tx, o_rx) = mpsc::channel::<PeerMessage>(32);
            let peer_id = peer_id.clone();
            let info_hash = magnet.info_hash.clone();
            let tx = tx.clone();
            let handle = tokio::spawn(async move {
                let conn_result = PeerConnection::connect(peer, peer_id, info_hash, tx, o_rx).await;
                match conn_result {
                    Ok(mut conn) => {
                        match conn.handle_messages().await {
                            Ok(_) => println!("Connection exited gracefully"),
                            Err(e) => println!("Connection exited with error: {:#}", e),
                        };
                    }
                    Err(e) => println!("Connection Failure: {:#}", e),
                };
            });
            send_channels.insert(peer, o_tx);
            handles.push(handle);
        }

        let mut sent = false;
        // Main execution loop
        let mut connected_peers = HashMap::<SocketAddr, Peer>::new();
        loop {
            if let Some(message) = rx.recv().await {
                // Start tracking peer on first message
                if !connected_peers.contains_key(&message.addr) {
                    let send_channel = send_channels.remove(&message.addr).unwrap();
                    connected_peers.entry(message.addr).or_insert(Peer {
                        addr: message.addr,
                        sender: send_channel,
                        state: PeerState::default(),
                    });
                    println!("{} peers connected", connected_peers.keys().len());
                }

                let peer = connected_peers.get_mut(&message.addr).unwrap();
                peer.state.update(message);

                let unchoked_peers = connected_peers
                    .values()
                    .filter(|p| !p.state.am_choked)
                    .collect::<Vec<&Peer>>();
                if !sent {
                    if let Some((piece, peer)) = Magdl::select_next_piece(unchoked_peers) {
                        let mut payload = [0u8; 12];
                        BigEndian::write_u32(&mut payload[0..4], piece as u32);
                        BigEndian::write_u32(&mut payload[4..8], 0);
                        BigEndian::write_u32(&mut payload[8..12], 2^14);
                        dbg!(&payload);
                        peer.sender
                            .send(PeerMessage {
                                message_type: PeerMessageType::Request,
                                payload: payload.to_vec(),
                                addr: peer.addr,
                            })
                            .await?;
                        sent = true;
                    }
                }
            }
        }
    }
    fn select_next_piece(peers: Vec<&Peer>) -> Option<(usize, &Peer)> {
        if let Some(&peer) = peers.first() {
            if let Some(piece) = peer.state.bitfield.iter().position(|&e| e) {
                return Some((piece, peer));
            }
        }
        None
    }
}
