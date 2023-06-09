//! MAGDL
//!
//! Minimal bittorrent download client
//!

struct Magdl;
impl Magdl {
    pub fn download(magnet_link: String) {
        // Parse magnet link into trackers and info hash
        todo!();

        // Initialize trackers and construct list of available peers
        todo!();

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
