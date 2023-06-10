//! MAGDL
//!
//! Minimal bittorrent download client
//!

use std::{
    borrow::BorrowMut,
    collections::HashMap,
    net::{self, IpAddr, Ipv4Addr, SocketAddr, TcpStream, ToSocketAddrs, UdpSocket},
    sync::{mpsc, Arc, Mutex, MutexGuard},
    thread,
    time::Duration, io::Write,
};

use anyhow::{bail, Context};
use byteorder::{BigEndian, ByteOrder};
use popol::{interest, Event};
use rand::{random, Rng};

#[derive(Eq, PartialEq, Clone, Hash, Debug)]
enum SourceKey {
    Peer(SocketAddr),
}

pub struct Magdl;
impl Magdl {
    pub fn download(magnet_link: String) -> anyhow::Result<()> {
        // Parse magnet link into trackers and info hash
        let magnet = Magnet::from_link_string(magnet_link);

        // Initialize trackers and construct list of available peers
        let tracker_connections: Vec<TrackerConnection> = magnet
            .tracker_urls
            .into_iter()
            .filter_map(|url| match TrackerConnection::establish(url) {
                Ok(conn) => {
                    println!("Connected to: {}", conn.url);
                    Some(conn)
                }
                Err(e) => {
                    println!("{}", e);
                    None
                }
            })
            .collect();

        let mut peer_id = [0u8; 20];
        rand::thread_rng().fill(&mut peer_id[..]);
        let signature = "-WM0001-";
        peer_id[0..signature.len()].copy_from_slice(signature.as_bytes());

        let peer_addrs: Vec<SocketAddr> = tracker_connections
            .iter()
            .filter_map(|tracker| {
                let announce_rd = AnnounceRequestDescriptor {
                    connection_id: tracker.connection_id,
                    peer_id: peer_id.clone(),
                    info_hash: magnet.info_hash.clone(),
                    downloaded: 0,
                    left: 0,
                    uploaded: 0,
                    event: AnnounceEvent::None,
                };
                match tracker.list_peer_addrs(announce_rd) {
                    Ok(peers) => Some(peers),
                    Err(_) => {
                        println!("Failed to retrieve peer list from tracker");
                        None
                    }
                }
            })
            .flatten()
            .collect();
        dbg!("Retrieved {} peer addresses", peer_addrs.len());

        // Connect to as many peers as possible and perform handshake
        let sources = Arc::new(Mutex::new(popol::Sources::new()));
        let events = Arc::new(Mutex::new(Vec::<Event<SourceKey>>::new()));
        let source_map = Arc::new(Mutex::new(HashMap::new()));

        let p_sources = sources.clone();
        let p_source_map = source_map.clone();
        let peer_thread = thread::spawn(move || {
            peer_addrs
                .into_iter()
                .for_each(|addr| match PeerConnection::establish(addr) {
                    Ok(conn) => {
                        let key = SourceKey::Peer(conn.addr);
                        p_sources.lock().unwrap().register(
                            key.clone(),
                            &conn.socket,
                            interest::ALL,
                        );
                        p_source_map.lock().unwrap().insert(key.clone(), conn);
                    }
                    Err(e) => println!("Failed to connect to peer {}", e),
                })
        });

        // Main execution loop
        loop {
            {
                let events: &mut Vec<_> = &mut events.lock().unwrap();
                sources.lock().unwrap().wait(events)?;
            }

            for event in events.lock().unwrap().drain(..) {
                let key = event.key;
                let source = event.source;
                if source.is_error() || source.is_invalid() || source.is_hangup() {
                    sources.lock().unwrap().unregister(&key);
                    dbg!("Disconnected or errored", &key);
                }

                let peer_conn = source_map
                    .lock()
                    .unwrap()
                    .get_mut(&key)
                    .expect("Registered source has no peer counterpart");

                if event.source.is_readable() {
                    peer_conn.process_read_queue();
                }
                if event.source.is_writable() {
                    peer_conn.flush_write_queue();
                }
            }

            // Receive messages from peers and update their state
            // todo!();

            // Request pieces from peers
            // todo!();
        }
    }
}

struct Magnet {
    tracker_urls: Vec<url::Url>,
    info_hash: [u8; 20],
    display_name: String,
}
impl Magnet {
    fn from_link_string(value: String) -> Self {
        let decoded = urlencoding::decode(&value).expect("Failed to parse magnet link");
        let slice = &decoded[8..];
        let split = slice.split("&").collect::<Vec<_>>();

        let mut trackers = Vec::new();
        let mut exact_topic = [0u8; 20];
        let mut display_name = String::new();
        for item in split {
            let (id, value) = item.split_once("=").unwrap();
            match id {
                "xt" => {
                    let info_string = value[value.len() - 40..].as_bytes();
                    let bytes = hex::decode(info_string)
                        .expect("Failed to parse info hash from magnet link");
                    exact_topic.copy_from_slice(bytes.as_slice());
                }
                "dn" => {
                    display_name = String::from(value);
                }
                "tr" => {
                    use std::str::FromStr;
                    if let Some(tracker) = url::Url::from_str(value).ok() {
                        trackers.push(tracker);
                    }
                }
                &_ => (),
            }
        }
        Self {
            tracker_urls: trackers,
            info_hash: exact_topic,
            display_name,
        }
    }
}

struct TrackerConnection {
    socket: UdpSocket,
    pub connection_id: i64,
    pub url: url::Url,
}
impl TrackerConnection {
    pub fn establish(url: url::Url) -> anyhow::Result<Self> {
        let host_port = format!("{}:{}", url.host_str().unwrap(), url.port().unwrap_or(80));
        let socket_addr = host_port.to_socket_addrs()?.next().unwrap();

        // maybe only use one socket in the future
        let socket = UdpSocket::bind("0.0.0.0:0")?;
        socket.set_read_timeout(Some(Duration::from_secs(3)))?;
        UdpSocket::connect(&socket, socket_addr)?;

        const PROTOCOL_ID: i64 = 0x41727101980;
        let mut connection_request = [0u8; 16];
        let transaction_id = random::<u32>();
        BigEndian::write_i64(&mut connection_request[0..8], PROTOCOL_ID);
        BigEndian::write_u32(&mut connection_request[8..12], 0);
        BigEndian::write_u32(&mut connection_request[12..16], transaction_id);
        socket.send(&connection_request)?;

        let mut connection_response = [0u8; 16];

        let bytes_read = socket.recv(&mut connection_response)?;
        if bytes_read != 16 {
            bail!("Invalid number of bytes read from tracker");
        }
        let response_transaction_id = BigEndian::read_u32(&connection_response[4..8]);
        let connection_id = BigEndian::read_i64(&connection_response[8..16]);
        if transaction_id != response_transaction_id {
            bail!("Mismatched transaction ID returned from tracker");
        }
        Ok(Self {
            socket,
            connection_id,
            url,
        })
    }
    pub fn list_peer_addrs(
        &self,
        request_descriptor: AnnounceRequestDescriptor,
    ) -> anyhow::Result<Vec<SocketAddr>> {
        let request = AnnounceRequest::new(request_descriptor);
        self.socket.send(request.to_bytes().as_slice())?;
        let mut response_buf = [0u8; 4092];
        let bytes_read = self.socket.recv(&mut response_buf)?;
        if bytes_read == 4092 {
            println!("May have overrun announce response buffer");
        }
        let response = AnnounceResponse::from_bytes(&response_buf, bytes_read);
        Ok(response.peers)
    }
}

#[derive(Debug, Copy, Clone)]
pub enum AnnounceEvent {
    None = 0,
    Completed,
    Started,
    Stopped,
}

#[derive(Debug)]
struct AnnounceRequest {
    connection_id: i64,
    action: u32,
    transaction_id: u32,
    info_hash: [u8; 20],
    peer_id: [u8; 20],
    downloaded: u64,
    left: u64,
    uploaded: u64,
    event: AnnounceEvent,
    ip_address: u32,
    key: u32,
    num_want: i32,
    port: u16,
}

#[derive(Debug)]
pub struct AnnounceRequestDescriptor {
    pub connection_id: i64,
    pub peer_id: [u8; 20],
    pub info_hash: [u8; 20],
    pub downloaded: u64,
    pub left: u64,
    pub uploaded: u64,
    pub event: AnnounceEvent,
}

const ANNOUNCE_REQUEST_BYTES: usize = 98;
impl AnnounceRequest {
    fn new(descriptor: AnnounceRequestDescriptor) -> Self {
        Self {
            connection_id: descriptor.connection_id,
            action: 1,
            transaction_id: rand::random(),
            info_hash: descriptor.info_hash,
            peer_id: descriptor.peer_id,
            downloaded: descriptor.downloaded,
            left: descriptor.left,
            uploaded: descriptor.uploaded,
            event: descriptor.event,
            ip_address: 0,
            key: rand::random(),
            num_want: -1,
            port: 6881,
        }
    }
    fn to_bytes(&self) -> Vec<u8> {
        let mut bytes = vec![0u8; ANNOUNCE_REQUEST_BYTES];
        BigEndian::write_i64(&mut bytes[0..8], self.connection_id);
        BigEndian::write_u32(&mut bytes[8..12], self.action);
        BigEndian::write_u32(&mut bytes[12..16], self.transaction_id);
        bytes[16..36].copy_from_slice(&self.info_hash);
        bytes[36..56].copy_from_slice(&self.peer_id);
        BigEndian::write_u64(&mut bytes[56..64], self.downloaded);
        BigEndian::write_u64(&mut bytes[64..72], self.left);
        BigEndian::write_u64(&mut bytes[72..80], self.uploaded);
        BigEndian::write_u32(&mut bytes[80..84], self.event as u32);
        BigEndian::write_u32(&mut bytes[84..88], self.ip_address);
        BigEndian::write_u32(&mut bytes[88..92], self.key);
        BigEndian::write_i32(&mut bytes[92..96], self.num_want);
        BigEndian::write_u16(&mut bytes[96..98], self.port);
        bytes
    }
}

#[derive(Debug)]
struct AnnounceResponse {
    action: u32,
    transaction_id: u32,
    interval: u32,
    leechers: u32,
    seeders: u32,
    peers: Vec<SocketAddr>,
}
impl AnnounceResponse {
    fn from_bytes(bytes: &[u8], length: usize) -> Self {
        let action = BigEndian::read_u32(&bytes[0..4]);
        let transaction_id = BigEndian::read_u32(&bytes[4..8]);
        let interval = BigEndian::read_u32(&bytes[8..12]);
        let leechers = BigEndian::read_u32(&bytes[12..16]);
        let seeders = BigEndian::read_u32(&bytes[16..20]);
        let peer_list = &bytes[20..length];
        if peer_list.len() % 6 != 0 {
            panic!("Invalid peer list size");
        }
        let mut peers = Vec::new();
        for address in peer_list.chunks(6) {
            let ip = Ipv4Addr::new(address[0], address[1], address[2], address[3]);
            let port = BigEndian::read_u16(&address[4..6]);
            let peer = SocketAddr::new(IpAddr::V4(ip), port);
            peers.push(peer);
        }
        Self {
            action,
            transaction_id,
            interval,
            leechers,
            seeders,
            peers,
        }
    }
}

struct PeerConnection {
    socket: TcpStream,
    addr: SocketAddr,
    write_queue: Vec<RawMessage>,
    read_queue: Vec<RawMessage>,
}
impl PeerConnection {
    fn establish(addr: SocketAddr) -> anyhow::Result<Self> {
        let socket = TcpStream::connect_timeout(&addr, Duration::from_secs(1))?;
        Ok(Self {
            socket,
            addr,
            write_queue: Vec::new(),
            read_queue: Vec::new(),
        })
    }

    fn flush_write_queue(&mut self) -> anyhow::Result<()> {
        let mut errors = Vec::new();
        for message in self.write_queue.drain(..) {
            let bytes: Vec<u8> = message.into();
            let result = self.socket.write(&bytes);
            if result.is_err() {
                errors.push(result.err());
            }
        }
        if errors.len() > 0 {
            bail!("Message failed to write")
        }
        Ok(())
    }

    fn process_read_queue(&self) -> anyhow::Result<()> {
        Ok(())
    }
}

struct RawMessage {
    message_id: u8,
    payload: Vec<u8>,
}
impl From<&[u8]> for RawMessage {
    fn from(bytes: &[u8]) -> Self {
        if bytes.len() == 0 {
            return Self {
                message_id: 0,
                payload: Vec::new(),
            };
        }
        let payload_length = bytes.len() - 1 as usize;
        let message_id = BigEndian::read_int(&bytes, 1) as u8;
        let mut payload = vec![0u8; payload_length];
        payload.copy_from_slice(&bytes[1..]);
        Self {
            message_id,
            payload,
        }
    }
}
impl From<RawMessage> for Vec<u8> {
    fn from(raw_message: RawMessage) -> Self {
        let mut bytes = vec![0u8; raw_message.payload.len() + 1];
        bytes[0] = raw_message.message_id;
        bytes[1..].copy_from_slice(&raw_message.payload);
        bytes
    }
}
