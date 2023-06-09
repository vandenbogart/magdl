//! MAGDL
//!
//! Minimal bittorrent download client
//!

use std::{net::{self, ToSocketAddrs, UdpSocket}, time::Duration};

use anyhow::{Context, bail};
use byteorder::{BigEndian, ByteOrder};
use rand::random;



pub struct Magdl;
impl Magdl {
    pub fn download(magnet_link: String) -> anyhow::Result<()> {
        // Parse magnet link into trackers and info hash
        let magnet = Magnet::from_link_string(magnet_link);

        // Initialize trackers and construct list of available peers
        let tracker_connections: Vec<TrackerConnection> = magnet.tracker_urls.into_iter().filter_map(|url| {
            match TrackerConnection::establish(url) {
                Ok(conn) => {
                    println!("Connected to: {}", conn.url);
                    Some(conn)
                },
                Err(e) => { println!("{}", e); None },
            }
        }).collect();

        // Connect to as many peers as possible and perform handshake
        todo!();

        // Main execution loop
        loop {
            // Receive messages from peers and update their state
            todo!();

            // Request pieces from peers
            todo!();
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
                    let bytes = hex::decode(info_string).expect("Failed to parse info hash from magnet link");
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
}



