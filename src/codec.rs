use std::pin::Pin;
use std::task::Poll;

use bytes::{BytesMut, BufMut};
use futures::Stream;
use pin_project::pin_project;
use tokio::io::{AsyncRead, ReadBuf};

pub trait Decoder {
    type Item;
    type Error: From<std::io::Error>;

    fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, std::io::Error>;
    fn framed<T: AsyncRead>(self, io: T) -> Framed<T, Self>
    where
        Self: Sized;
}

pub trait Encoder<Item> {
    type Error: From<std::io::Error>;

    fn encode(item: Item, dst: &mut BytesMut) -> Result<(), std::io::Error>;
}

#[pin_project]
pub struct Framed<T, U> {
    #[pin]
    inner: T,
    codec: U,
    read: BytesMut,
}

impl<T, U> Framed<T, U>
where
    T: AsyncRead,
{
    pub fn new(inner: T, codec: U) -> Framed<T, U> {
        Self {
            inner,
            codec,
            read: BytesMut::new(),
        }
    }
}

impl<T, U> Stream for Framed<T, U>
where
    T: AsyncRead,
    U: Decoder,
{
    type Item = U::Item;

    fn poll_next(
        self: Pin<&mut Self>,
        cx: &mut std::task::Context<'_>,
    ) -> Poll<Option<Self::Item>> {
        let mut pinned = self.project();
        match pinned.codec.decode(&mut pinned.read) {
            Ok(Some(frame)) => {
                return Poll::Ready(Some(frame));
            }
            Ok(None) => {
                // Continue on to read
                ()
            }
            Err(e) => {
                dbg!(e);
                // Could not decode, stream is invalid
                // in future could just ignore message?
                return Poll::Ready(None);
            }
        };

        let mut a = [0u8; 1024];
        let rb = &mut ReadBuf::new(&mut a);
        match pinned.inner.poll_read(cx, rb) {
            Poll::Ready(Err(e)) => {
                dbg!(e);

                // TODO: Handle errors somehow
                Poll::Ready(None)
            }
            Poll::Ready(Ok(_)) => {
                // read 0 or more bytes
                let len = rb.filled().len();
                if len == 0 {
                    // EOF
                    Poll::Ready(None)
                } else {
                    pinned.read.put(rb.filled());
                    match pinned.codec.decode(&mut pinned.read) {
                        Ok(Some(frame)) => {
                            return Poll::Ready(Some(frame));
                        }
                        Ok(None) => {
                            return Poll::Ready(None);
                        }
                        Err(e) => {
                            dbg!(e);
                            // Could not decode, stream is invalid
                            // in future could just ignore message?
                            return Poll::Ready(None);
                        }
                    }
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

#[cfg(test)]
mod tests {
    use byteorder::{LittleEndian, ByteOrder};
    use bytes::Buf;
    use futures::StreamExt;

    use super::*;

    #[pin_project]
    struct MockTcpStream {
        #[pin]
        read: Vec<u8>,
        #[pin]
        write: Vec<u8>,
    }
    impl AsyncRead for MockTcpStream {
        fn poll_read(
            self: Pin<&mut Self>,
            cx: &mut std::task::Context<'_>,
            buf: &mut ReadBuf<'_>,
        ) -> Poll<std::io::Result<()>> {
            let mut pinned = self.project();
            buf.put_slice(pinned.read.as_slice());
            pinned.read.clear();
            Poll::Ready(Ok(()))
        }
    }

    struct MockDecoder {}
    impl Decoder for MockDecoder {
        type Item = u32;

        type Error = std::io::Error;

        fn decode(&mut self, buf: &mut BytesMut) -> Result<Option<Self::Item>, std::io::Error> {
            if buf.len() < 4 {
                return Ok(None);
            }
            let number = LittleEndian::read_u32(buf);
            buf.advance(4);
            if number == 0 {
                return Err(std::io::Error::from(std::io::ErrorKind::InvalidData))
            }
            Ok(Some(number))
        }

        fn framed<T: AsyncRead>(self, io: T) -> Framed<T, Self>
        where
            Self: Sized,
        {
            Framed::new(io, self)
        }
    }

    #[tokio::test]
    async fn test_read_frame() {
        let mut read = BytesMut::new();
        let expected = [11, 2, 5];
        for val in expected.iter() {
            read.put_u32_le(*val);
        }
        let tcp_stream = MockTcpStream {
            read: read.to_vec(),
            write: Vec::new()
        };
        let decoder = MockDecoder {};
        let mut framed = decoder.framed(tcp_stream);
        let mut expected_iter = expected.iter();
        while let Some(frame) = framed.next().await {
            assert_eq!(frame, *expected_iter.next().unwrap());
        }
    }
}
