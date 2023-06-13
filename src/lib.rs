mod tracker_stream;
mod peer_connection;
mod connection;
//mod trackers;
mod magnet;
use futures::{stream::FuturesUnordered, StreamExt};
use magnet::Magnet;
use rand::Rng;
use tracker_stream::Trackers;

use crate::peer_connection::PeerConnection;
// use trackers::Trackers;

pub struct Magdl;
impl Magdl {
    pub async fn download(magnet_link: String) -> anyhow::Result<()> {
        let magnet = Magnet::from_link_string(magnet_link);
        let trackers = Trackers::new(&magnet.tracker_urls).await;
        let mut peer_id = [0u8; 20];
        rand::thread_rng().fill(&mut peer_id[..]);
        let signature = "-WM0001-";
        peer_id[0..signature.len()].copy_from_slice(signature.as_bytes());
        println!("Peer ID: {}", String::from_utf8_lossy(&peer_id));
        println!("Info Hash: {}", String::from_utf8_lossy(&magnet.info_hash));

        let peers = trackers.announce(peer_id, magnet.info_hash).await;
        let mut handles = FuturesUnordered::new();
        for peer in peers.into_iter() {
            let handle = tokio::spawn(PeerConnection::connect(peer, peer_id.clone(), magnet.info_hash.clone()));
            handles.push(handle);
        }
        while let Some(result) = handles.next().await {
            match result.unwrap() {
                Ok(_) => (),
                Err(e) => println!("{:#}", e),
            }

        }
        Ok(())
    }
}


