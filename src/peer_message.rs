use bytes::Bytes;

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
    pub payload: Bytes,
}
