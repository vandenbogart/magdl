use std::{
    fmt::{Debug, Display},
    net::SocketAddr,
};

use anyhow::Context;
use byteorder::{BigEndian, ByteOrder};
use tokio::sync::{mpsc, oneshot};

use crate::connection::{Frame, TcpConnection};

#[derive(Debug, Eq, PartialEq)]
pub enum PeerMessageType {
    Choke,
    Unchoke,
    Interested,
    NotInterested,
    Have,
    Bitfield,
    Request,
    Piece,
    Cancel,
    Port,
}
impl PeerMessageType {
    pub fn raw_value(&self) -> u8 {
        match self {
            PeerMessageType::Choke => 0,
            PeerMessageType::Unchoke => 1,
            PeerMessageType::Interested => 2,
            PeerMessageType::NotInterested => 3,
            PeerMessageType::Have => 4,
            PeerMessageType::Bitfield => 5,
            PeerMessageType::Request => 6,
            PeerMessageType::Piece => 7,
            PeerMessageType::Cancel => 8,
            PeerMessageType::Port => 9,
            _ => panic!("Invalid peer message type"),
        }
    }
}
impl From<u8> for PeerMessageType {
    fn from(value: u8) -> Self {
        match value {
            0 => Self::Choke,
            1 => Self::Unchoke,
            2 => Self::Interested,
            3 => Self::NotInterested,
            4 => Self::Have,
            5 => Self::Bitfield,
            6 => Self::Request,
            7 => Self::Piece,
            8 => Self::Cancel,
            9 => Self::Port,
            _ => panic!("Invalid peer message type"),
        }
    }
}

#[derive(Debug)]
pub struct PeerMessage {
    pub message_type: PeerMessageType,
    pub payload: Vec<u8>,
    pub addr: SocketAddr,
}

const BITTORRENT_PROTOCOL: &str = "BitTorrent protocol";
pub struct PeerConnection {
    tcp_conn: TcpConnection,
    addr: SocketAddr,
    sender: mpsc::Sender<PeerMessage>,
    receiver: mpsc::Receiver<PeerMessage>,
}
impl PeerConnection {
    pub async fn connect(
        addr: SocketAddr,
        peer_id: [u8; 20],
        info_hash: [u8; 20],
        sender: mpsc::Sender<PeerMessage>,
        receiver: mpsc::Receiver<PeerMessage>,
    ) -> anyhow::Result<Self> {
        let mut conn = TcpConnection::establish(addr).await?;
        let handshake = Handshake {
            pstr: BITTORRENT_PROTOCOL.as_bytes().to_vec(),
            info_hash,
            peer_id,
        };
        conn.write(&[PeerFrame::Handshake(handshake)])
            .await
            .context("Writing handshake to peer")?;
        let mut frames = conn.read::<PeerFrame>().await?.into_iter();
        if let Some(PeerFrame::Handshake(hs)) = frames.next() {
            if hs.info_hash != info_hash {
                anyhow::bail!("Bad infohash in handshake");
            }
        }
        while let Some(frame) = frames.next() {
            match frame {
                PeerFrame::Handshake(_) => anyhow::bail!("Multiple handshake responses"),
                PeerFrame::Data(d) => {
                    sender
                        .send(PeerMessage {
                            message_type: PeerMessageType::from(d.message_id),
                            payload: d.payload,
                            addr,
                        })
                        .await?
                },
                PeerFrame::Disconnected => todo!(),
            };
        }

        Ok(Self {
            tcp_conn: conn,
            addr,
            sender,
            receiver,
        })
    }
    pub async fn handle_messages(&mut self) -> anyhow::Result<()> {
        loop {
            println!("try read: {}", self.addr);
            let frames = match self.tcp_conn.try_read::<PeerFrame>().await {
                Ok(frames) => frames,
                Err(_) => {
                    let frames = vec![PeerFrame::Disconnected];
                    frames
                },
            };
            for frame in frames.into_iter() {
                match frame {
                    PeerFrame::Handshake(_) => anyhow::bail!("Multiple handshake responses"),
                    PeerFrame::Data(d) => {
                        println!("try forward: {}", self.addr);
                        self.sender
                            .send(PeerMessage {
                                message_type: PeerMessageType::from(d.message_id),
                                payload: d.payload,
                                addr: self.addr,
                            })
                            .await?;
                        println!("done forward: {}", self.addr);
                    },
                    PeerFrame::Disconnected => {
                        self.sender
                            .send(PeerMessage {
                                message_type: PeerMessageType::Cancel,
                                payload: Vec::new(),
                                addr: self.addr,
                            })
                            .await?;
                        anyhow::bail!("disconnected");
                    },
                };
            }

            println!("done read: {}", self.addr);
            println!("try receive: {}", self.addr);
            match self.receiver.try_recv() {
                Ok(message) => {
                    println!("done receive: {}", self.addr);
                    let frame = PeerFrame::Data(Data {
                        message_id: message.message_type.raw_value(),
                        payload: message.payload,
                    });
                    println!("try send: {}", self.addr);
                    self.tcp_conn.write(&[frame]).await?;
                    println!("done send: {}", self.addr);
                }
                Err(mpsc::error::TryRecvError::Empty) => continue,
                Err(mpsc::error::TryRecvError::Disconnected) => {
                    anyhow::bail!("oneshot channel closed")
                }
            };
        }
    }
}

#[derive(Debug)]
pub struct Handshake {
    pstr: Vec<u8>,
    info_hash: [u8; 20],
    peer_id: [u8; 20],
}
impl Display for Handshake {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_fmt(format_args!(
            "Handshake:\npstr: {}\ninfo_hash: {}\npeer_id: {}\n",
            String::from_utf8_lossy(&self.pstr),
            String::from_utf8_lossy(&self.info_hash),
            String::from_utf8_lossy(&self.peer_id)
        ))
    }
}
impl Handshake {
    fn from_bytes(bytes: &[u8]) -> anyhow::Result<(Self, usize)> {
        if bytes.len() <= 68 {
            anyhow::bail!("Failed to parse handshake")
        }
        let pstrlen = BigEndian::read_int(bytes, 1) as usize;
        if pstrlen != 19 {
            anyhow::bail!("Failed to parse handshake")
        }
        let mut pstr = vec![0u8; pstrlen];
        pstr.copy_from_slice(&bytes[1..1 + pstrlen]);
        if pstr != BITTORRENT_PROTOCOL.as_bytes() {
            anyhow::bail!("Failed to parse handshake")
        }
        let mut info_hash = [0u8; 20];
        let end_reserved = 9 + pstrlen;
        info_hash.copy_from_slice(&bytes[end_reserved..end_reserved + 20]);
        let mut peer_id = [0u8; 20];
        peer_id.copy_from_slice(&bytes[end_reserved + 20..end_reserved + 40]);
        let handshake = Self {
            pstr,
            info_hash,
            peer_id,
        };
        Ok((handshake, 49 + pstrlen))
    }
    fn to_bytes(&self) -> Vec<u8> {
        let pstrlen = self.pstr.len();
        let size = 1 + pstrlen + 8 + 20 + 20;
        let mut bytes = vec![0u8; size];
        BigEndian::write_int(&mut bytes, self.pstr.len() as i64, 1);
        bytes[1..1 + pstrlen].copy_from_slice(&self.pstr);
        // pad reserved 8 bytes
        let end_res = 9 + pstrlen;
        bytes[end_res..20 + end_res].copy_from_slice(&self.info_hash);
        bytes[20 + end_res..40 + end_res].copy_from_slice(&self.peer_id);
        bytes.to_vec()
    }
}

#[derive(Debug)]
pub struct Data {
    message_id: u8,
    payload: Vec<u8>,
}
impl Data {
    fn from_bytes(bytes: &[u8]) -> anyhow::Result<(Self, usize)> {
        if bytes.len() < 4 {
            anyhow::bail!("Not enough bytes");
        }
        let mut cur = 0;
        let message_len = BigEndian::read_u32(&bytes[0..4]);
        cur += 4;
        if message_len == 0 {
            return Ok((
                Self {
                    message_id: 0,
                    payload: Vec::new(),
                },
                cur,
            ));
        }
        if bytes.len() < 5 {
            anyhow::bail!("Not enough bytes");
        }
        let message_id = BigEndian::read_int(&bytes[4..5], 1) as u8;
        cur += 1;
        if message_id > 9 {
            anyhow::bail!("Bad message id");
        }
        let payload_size: usize = message_len as usize - 1;
        if cur + payload_size > bytes.len() {
            anyhow::bail!("Not enough payload");
        }
        let payload = bytes[cur..cur + payload_size].to_vec();
        cur += payload_size;
        Ok((
            Self {
                message_id,
                payload,
            },
            cur,
        ))
    }
    fn to_bytes(&self) -> Vec<u8> {
        let message_len = 1 + self.payload.len();
        let mut bytes = vec![0u8; 4 + message_len];
        BigEndian::write_u32(&mut bytes[0..4], message_len as u32);
        BigEndian::write_int(&mut bytes[4..5], self.message_id.into(), 1);
        bytes[5..].copy_from_slice(&self.payload);
        bytes.to_vec()
    }
}

#[derive(Debug)]
pub enum PeerFrame {
    Handshake(Handshake),
    Data(Data),
    Disconnected,
}

impl Frame for PeerFrame {
    fn from_bytes(bytes: &[u8]) -> Option<(Self, usize)>
    where
        Self: Sized,
    {
        if let Some((handshake, size)) = Handshake::from_bytes(bytes).ok() {
            return Some((PeerFrame::Handshake(handshake), size));
        } else if let Some((data, size)) = Data::from_bytes(bytes).ok() {
            return Some((PeerFrame::Data(data), size));
        } else {
            return None;
        }
    }

    fn to_bytes(&self) -> Vec<u8> {
        match self {
            PeerFrame::Handshake(v) => v.to_bytes(),
            PeerFrame::Data(v) => v.to_bytes(),
            PeerFrame::Disconnected => todo!(),
        }
    }
}
