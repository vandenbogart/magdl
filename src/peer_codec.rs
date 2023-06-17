use bytes::BytesMut;
use tokio::io::AsyncRead;

use crate::codec::{Decoder, Framed, Encoder};


pub struct PeerMessage {}

pub struct PeerCodec {}
impl PeerCodec {
    pub fn new() -> Self {
        Self {}
    }
}

impl Decoder for PeerCodec {
    type Item = PeerMessage;
    type Error = std::io::Error;
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<PeerMessage>, std::io::Error> {
        todo!();
    }
    fn framed<T: AsyncRead>(self, io: T) -> Framed<T, Self> {
        todo!();
    }
}
impl Encoder<PeerMessage> for PeerCodec {
    type Error = std::io::Error;

    fn encode(item: PeerMessage, dst: &mut BytesMut) -> Result<(), std::io::Error> {
        todo!()
    }
}

