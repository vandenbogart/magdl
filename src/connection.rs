use std::{net::SocketAddr, io};

use anyhow::Context;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpStream,
};

pub trait Frame {
    fn from_bytes(bytes: &[u8]) -> Option<(Self, usize)>
    where
        Self: Sized;
    fn to_bytes(&self) -> Vec<u8>;
}

pub struct TcpConnection {
    tcp_stream: TcpStream,
    r_cur: usize,
    read_buf: Vec<u8>,
}

impl TcpConnection {
    pub async fn establish(addr: SocketAddr) -> anyhow::Result<Self> {
        let tcp_stream = TcpStream::connect(addr).await.context("Connecting to peer: ")?;
        Ok(Self {
            tcp_stream,
            r_cur: 0,
            read_buf: vec![0u8; 4096],
        })
    }
    pub async fn read<T: Frame + Sized>(&mut self) -> anyhow::Result<Vec<T>> {
        match self.tcp_stream.read(&mut self.read_buf).await {
            Ok(0) => {
                anyhow::bail!("Connection reset by peer");
            }
            Ok(n) => {
                self.r_cur += n;
                if self.r_cur == self.read_buf.len() {
                    self.read_buf.resize(self.read_buf.len() * 2, 0u8);
                }
            }
            Err(e) => anyhow::bail!(e),
        }

        let mut frames = Vec::new();
        while let Some((frame, len)) = T::from_bytes(&self.read_buf[..self.r_cur]) {
            let (_, remain) = self.read_buf.split_at(len);
            self.r_cur -= len;
            self.read_buf = remain.to_vec();
            frames.push(frame);
        }
        Ok(frames)
    }
    pub async fn try_read<T: Frame + Sized>(&mut self) -> anyhow::Result<Vec<T>> {
        self.tcp_stream.readable().await?;
        match self.tcp_stream.try_read(&mut self.read_buf) {
            Ok(0) => {
                anyhow::bail!("Connection reset by peer");
            }
            Ok(n) => {
                self.r_cur += n;
                if self.r_cur == self.read_buf.len() {
                    self.read_buf.resize(self.read_buf.len() * 2, 0u8);
                }
            }
            Err(ref e) if e.kind() == io::ErrorKind::WouldBlock => (),
            Err(e) => anyhow::bail!(e),
        }

        let mut frames = Vec::new();
        while let Some((frame, len)) = T::from_bytes(&self.read_buf[..self.r_cur]) {
            let (_, remain) = self.read_buf.split_at(len);
            self.r_cur -= len;
            self.read_buf = remain.to_vec();
            frames.push(frame);
        }
        Ok(frames)
    }
    pub async fn write<T: Frame + Sized>(&mut self, frames: &[T]) -> anyhow::Result<()> {
        let bytes: Vec<u8> = frames.iter().flat_map(|f| f.to_bytes()).collect();
        self.tcp_stream.write_all(&bytes).await.context("Failed to write to peer: ")
    }
}
