use std::fmt::Display;

use bytes::{Buf, BufMut, Bytes, BytesMut};
use tokio::io::{AsyncRead, AsyncWrite};
use tokio_util::codec::{Decoder, Encoder, Framed};

pub enum PeerFrame {
    Handshake(Handshake),
    Data(Data),
}

pub const BITTORRENT_PROTOCOL: &str = "BitTorrent protocol";
#[derive(Debug)]
pub struct Handshake {
    pub pstr: Bytes,
    pub info_hash: Bytes,
    pub peer_id: Bytes,
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
    fn decode(bytes: &mut BytesMut) -> Result<Option<Self>, std::io::Error> {
        if bytes.remaining() < 68 {
            return Ok(None);
        }
        let backup = bytes.clone();
        let pstrlen = bytes.get_u8();
        if pstrlen != 19 {
            *bytes = backup;
            return Err(std::io::ErrorKind::Unsupported.into());
        }
        let pstr = bytes.split_to(pstrlen as usize);
        if pstr != BITTORRENT_PROTOCOL.as_bytes() {
            *bytes = backup;
            return Err(std::io::ErrorKind::Unsupported.into());
        }
        bytes.get_int(8);
        let info_hash = bytes.split_to(20);
        let peer_id = bytes.split_to(20);
        let handshake = Self {
            pstr: pstr.into(),
            info_hash: info_hash.into(),
            peer_id: peer_id.into(),
        };
        Ok(Some(handshake))
    }

    fn encode(&self) -> Bytes {
        let mut bytes = BytesMut::new();
        let pstrlen = self.pstr.len() as u8;
        bytes.put_u8(pstrlen);
        bytes.put(self.pstr.clone());
        bytes.put_int(0, 8);
        bytes.put(self.info_hash.clone());
        bytes.put(self.peer_id.clone());
        bytes.into()
    }
}

#[derive(Debug)]
pub struct Data {
    pub message_id: u8,
    pub payload: Bytes,
}
impl Data {
    fn decode(bytes: &mut BytesMut) -> Result<Option<Self>, std::io::Error> {
        if bytes.remaining() < 4 {
            return Ok(None);
        }
        let backup = bytes.clone();
        let message_len = bytes.get_u32();
        if message_len == 0 {
            return Ok(Some(Self {
                message_id: 0,
                payload: Bytes::new(),
            }));
        }
        if bytes.remaining() < message_len as usize {
            *bytes = backup;
            return Ok(None);
        }
        let message_id = bytes.get_u8();
        let payload = bytes.split_to(message_len as usize - 1);
        Ok(Some(Self {
            message_id,
            payload: payload.into(),
        }))
    }
    fn encode(&self) -> Bytes {
        let message_len = 1 + self.payload.len();
        let mut bytes = BytesMut::new();
        bytes.put_u32(message_len as u32);
        bytes.put_u8(self.message_id);
        bytes.put(self.payload.clone());
        bytes.into()
    }
}

pub struct PeerCodec {}

impl PeerCodec {
    pub fn new() -> Self {
        Self {}
    }
}

impl Decoder for PeerCodec {
    type Item = PeerFrame;
    type Error = std::io::Error;
    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if let Some(data) = Data::decode(buf)? {
            return Ok(Some(PeerFrame::Data(data)));
        } else if let Some(handshake) = Handshake::decode(buf)? {
            return Ok(Some(PeerFrame::Handshake(handshake)));
        }
        Ok(None)
    }

    fn decode_eof(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        match self.decode(buf)? {
            Some(frame) => Ok(Some(frame)),
            None => {
                if buf.is_empty() {
                    Ok(None)
                } else {
                    Err(
                        std::io::Error::new(std::io::ErrorKind::Other, "bytes remaining on stream")
                            .into(),
                    )
                }
            }
        }
    }

    fn framed<T: AsyncRead + AsyncWrite + Sized>(self, io: T) -> Framed<T, Self>
    where
        Self: Sized,
    {
        Framed::new(io, self)
    }
}
impl Encoder<PeerFrame> for PeerCodec {
    type Error = std::io::Error;

    fn encode(&mut self, item: PeerFrame, dst: &mut BytesMut) -> Result<(), Self::Error> {
        match item {
            PeerFrame::Data(i) => dst.put(i.encode()),
            PeerFrame::Handshake(i) => dst.put(i.encode()),
        };
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_decode_handshake() {
        let mut codec = PeerCodec::new();
        let mut bytes = BytesMut::new();
        bytes.put_u8(19);
        bytes.put(BITTORRENT_PROTOCOL.as_bytes());
        bytes.put_int(0, 8);
        let info_hash = vec![1u8; 20];
        let peer_id = vec![2u8; 20];
        bytes.put(info_hash.as_slice());
        bytes.put(peer_id.as_slice());
        if let PeerFrame::Handshake(hs) = codec.decode(&mut bytes).unwrap().unwrap() {
            assert_eq!(hs.pstr, BITTORRENT_PROTOCOL.as_bytes());
            assert_eq!(hs.info_hash, info_hash);
            assert_eq!(hs.peer_id, peer_id);
        } else {
            assert!(false);
        }
    }

    #[test]
    fn test_decode_data() {
        let mut codec = PeerCodec::new();
        let mut bytes = BytesMut::new();
        bytes.put_u32(20);
        bytes.put_u8(5);
        let payload = vec![1u8; 19];
        bytes.put(payload.as_slice());
        if let PeerFrame::Data(d) = codec.decode(&mut bytes).unwrap().unwrap() {
            assert_eq!(d.message_id, 5);
            assert_eq!(d.payload, vec![1u8; 19]);
        } else {
            assert!(false);
        }
    }
}
