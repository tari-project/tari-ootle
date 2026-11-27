//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::{Buf, BufMut, Bytes, BytesMut};
use futures::{AsyncRead, AsyncWrite};
use tokio::io::ReadBuf;
use tokio_util::codec::{Decoder, Encoder, Framed};

/// Canonical framing
pub type CanonicalFraming<T> = Framed<BoundedReadCompat<T>, RpcFrameCodec>;

/// Create a length-delimited frame around the given stream reader/writer with the given maximum frame length.
pub fn canonical<T>(stream: T, max_frame_len: usize) -> CanonicalFraming<T>
where T: AsyncRead + AsyncWrite + Unpin {
    Framed::new(BoundedReadCompat::new(stream), RpcFrameCodec::new(max_frame_len))
}

const LENGTH_PREFIX_LEN: usize = 4;

/// The most read-buffer capacity the decoder requests for a frame body in one step. The body length comes from the
/// remote peer, so read-buffer growth must track the bytes received.
const MAX_BODY_RESERVE: usize = 64 * 1024;

/// The most bytes [`BoundedReadCompat`] zero-fills and reads in one poll.
const MAX_READ_LEN: usize = 64 * 1024;

/// Adapts a `futures` stream to the `tokio` I/O traits that [`Framed`] requires.
///
/// A `futures` reader takes an initialised slice, so each poll zero-fills the slice it hands over. The read buffer's
/// spare capacity grows with the bytes already buffered, so the slice is capped at [`MAX_READ_LEN`] to keep the
/// per-poll cost constant however much of a frame has arrived.
#[derive(Debug)]
pub struct BoundedReadCompat<T> {
    inner: T,
}

impl<T> BoundedReadCompat<T> {
    fn new(inner: T) -> Self {
        Self { inner }
    }
}

impl<T: AsyncRead + Unpin> tokio::io::AsyncRead for BoundedReadCompat<T> {
    fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let len = buf.remaining().min(MAX_READ_LEN);
        let slice = buf.initialize_unfilled_to(len);
        let n = ready!(Pin::new(&mut self.inner).poll_read(cx, slice))?;
        buf.advance(n);
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> tokio::io::AsyncWrite for BoundedReadCompat<T> {
    fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.inner).poll_write(cx, buf)
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.inner).poll_close(cx)
    }
}

/// Frames each message as a 4-byte big-endian length followed by that many payload bytes.
#[derive(Debug, Clone, Copy)]
pub struct RpcFrameCodec {
    max_frame_len: usize,
}

impl RpcFrameCodec {
    pub fn new(max_frame_len: usize) -> Self {
        Self { max_frame_len }
    }
}

impl Decoder for RpcFrameCodec {
    type Error = io::Error;
    type Item = BytesMut;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        let Some(prefix) = src.get(..LENGTH_PREFIX_LEN) else {
            src.reserve(LENGTH_PREFIX_LEN - src.len());
            return Ok(None);
        };
        let mut prefix_bytes = [0u8; LENGTH_PREFIX_LEN];
        prefix_bytes.copy_from_slice(prefix);
        let frame_len = usize::try_from(u32::from_be_bytes(prefix_bytes)).unwrap_or(usize::MAX);
        if frame_len > self.max_frame_len {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "peer declared a {frame_len}-byte frame, which exceeds the {}-byte maximum",
                    self.max_frame_len
                ),
            ));
        }

        let frame_end = frame_len.saturating_add(LENGTH_PREFIX_LEN);
        if src.len() < frame_end {
            src.reserve((frame_end - src.len()).min(MAX_BODY_RESERVE));
            return Ok(None);
        }

        src.advance(LENGTH_PREFIX_LEN);
        Ok(Some(src.split_to(frame_len)))
    }
}

impl Encoder<Bytes> for RpcFrameCodec {
    type Error = io::Error;

    fn encode(&mut self, item: Bytes, dst: &mut BytesMut) -> Result<(), Self::Error> {
        let frame_len = item.len();
        let prefix = u32::try_from(frame_len)
            .ok()
            .filter(|_| frame_len <= self.max_frame_len)
            .ok_or_else(|| {
                io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!(
                        "cannot send a {frame_len}-byte frame, which exceeds the {}-byte maximum",
                        self.max_frame_len
                    ),
                )
            })?;
        dst.reserve(LENGTH_PREFIX_LEN + frame_len);
        dst.put_u32(prefix);
        dst.extend_from_slice(&item);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::{
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use futures::{SinkExt, StreamExt};
    use tokio::io::{AsyncWriteExt, DuplexStream};
    use tokio_util::compat::{Compat, TokioAsyncReadCompatExt};

    use super::*;

    const MAX_FRAME_LEN: usize = 6 * 1024 * 1024;

    /// Records the largest slice the framing hands to the stream in one read.
    struct ReadSizeRecorder {
        inner: Compat<DuplexStream>,
        largest_read: Arc<AtomicUsize>,
    }

    impl AsyncRead for ReadSizeRecorder {
        fn poll_read(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
            self.largest_read.fetch_max(buf.len(), Ordering::Relaxed);
            Pin::new(&mut self.inner).poll_read(cx, buf)
        }
    }

    impl AsyncWrite for ReadSizeRecorder {
        fn poll_write(mut self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            Pin::new(&mut self.inner).poll_write(cx, buf)
        }

        fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.inner).poll_flush(cx)
        }

        fn poll_close(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Pin::new(&mut self.inner).poll_close(cx)
        }
    }

    #[tokio::test]
    async fn each_read_initialises_a_bounded_slice_however_much_is_buffered() {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let largest_read = Arc::new(AtomicUsize::new(0));
        let stream = ReadSizeRecorder {
            inner: server.compat(),
            largest_read: largest_read.clone(),
        };
        let mut framed = canonical(stream, MAX_FRAME_LEN);

        let header = u32::try_from(MAX_FRAME_LEN).unwrap().to_be_bytes();
        client.write_all(&header).await.unwrap();
        let received = 3 * 1024 * 1024;
        let writer = tokio::spawn(async move {
            client.write_all(&vec![7u8; received]).await.unwrap();
            client
        });
        while framed.read_buffer().len() < LENGTH_PREFIX_LEN + received {
            let read = tokio::time::timeout(Duration::from_millis(20), framed.next()).await;
            assert!(read.is_err(), "a frame decoded before its body arrived");
        }
        let _client = writer.await.unwrap();

        let largest_read = largest_read.load(Ordering::Relaxed);
        assert!(
            largest_read <= 64 * 1024,
            "with {received} bytes buffered, one read initialised a {largest_read}-byte slice"
        );
    }

    /// Polls the framing until `len` bytes of an incomplete frame sit in its read buffer.
    async fn read_until_buffered(framed: &mut CanonicalFraming<Compat<tokio::io::DuplexStream>>, len: usize) {
        while framed.read_buffer().len() < len {
            let read = tokio::time::timeout(Duration::from_millis(20), framed.next()).await;
            assert!(read.is_err(), "a frame decoded before its body arrived");
        }
    }

    #[tokio::test]
    async fn header_alone_does_not_grow_read_buffer_to_declared_length() {
        let (mut client, server) = tokio::io::duplex(1024);
        let mut framed = canonical(server.compat(), MAX_FRAME_LEN);

        let header = u32::try_from(MAX_FRAME_LEN).unwrap().to_be_bytes();
        client.write_all(&header).await.unwrap();
        read_until_buffered(&mut framed, LENGTH_PREFIX_LEN).await;

        let capacity = framed.read_buffer().capacity();
        assert!(
            capacity < 1024 * 1024,
            "a 4-byte header grew the read buffer to {capacity} bytes"
        );
    }

    #[tokio::test]
    async fn read_buffer_grows_with_the_received_body() {
        let (mut client, server) = tokio::io::duplex(64 * 1024);
        let mut framed = canonical(server.compat(), MAX_FRAME_LEN);

        let header = u32::try_from(MAX_FRAME_LEN).unwrap().to_be_bytes();
        client.write_all(&header).await.unwrap();
        let received = 1024 * 1024;
        let writer = tokio::spawn(async move {
            client.write_all(&vec![7u8; received]).await.unwrap();
            client
        });
        read_until_buffered(&mut framed, LENGTH_PREFIX_LEN + received).await;
        let _client = writer.await.unwrap();

        let capacity = framed.read_buffer().capacity();
        assert!(
            capacity <= 3 * received,
            "{received} received bytes grew the read buffer to {capacity} bytes"
        );
    }

    #[tokio::test]
    async fn frames_round_trip_up_to_the_maximum_length() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let mut sender = canonical(client.compat(), MAX_FRAME_LEN);
        let mut receiver = canonical(server.compat(), MAX_FRAME_LEN);

        let frames = [
            Bytes::new(),
            Bytes::from_static(b"hello"),
            Bytes::from(vec![1u8; 100 * 1024]),
            Bytes::from(vec![2u8; MAX_FRAME_LEN]),
            Bytes::from_static(b"after"),
        ];
        let to_send = frames.clone();
        let send = tokio::spawn(async move {
            for frame in to_send {
                sender.send(frame).await.unwrap();
            }
            sender
        });

        for expected in &frames {
            let frame = receiver.next().await.unwrap().unwrap();
            assert_eq!(frame.len(), expected.len());
            assert_eq!(frame.freeze(), *expected);
        }
        let _sender = send.await.unwrap();
    }

    #[tokio::test]
    async fn body_written_in_small_pieces_decodes() {
        let (mut client, server) = tokio::io::duplex(16);
        let mut framed = canonical(server.compat(), MAX_FRAME_LEN);

        let body = (0..200_000u32).map(|i| (i % 251) as u8).collect::<Vec<_>>();
        let mut wire = u32::try_from(body.len()).unwrap().to_be_bytes().to_vec();
        wire.extend_from_slice(&body);
        let writer = tokio::spawn(async move {
            for piece in wire.chunks(7) {
                client.write_all(piece).await.unwrap();
            }
            client
        });

        let frame = framed.next().await.unwrap().unwrap();
        assert_eq!(&frame[..], &body[..]);
        let _client = writer.await.unwrap();
    }

    #[tokio::test]
    async fn declared_length_above_the_maximum_is_rejected() {
        let (mut client, server) = tokio::io::duplex(1024);
        let mut framed = canonical(server.compat(), MAX_FRAME_LEN);

        let header = u32::try_from(MAX_FRAME_LEN + 1).unwrap().to_be_bytes();
        client.write_all(&header).await.unwrap();
        let err = framed.next().await.unwrap().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[test]
    fn frame_above_the_maximum_is_not_encoded() {
        let mut codec = RpcFrameCodec::new(8);
        let mut dst = BytesMut::new();
        let err = codec.encode(Bytes::from_static(b"123456789"), &mut dst).unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidInput);
        assert!(dst.is_empty());

        codec.encode(Bytes::from_static(b"12345678"), &mut dst).unwrap();
        assert_eq!(&dst[..], b"\x00\x00\x00\x0812345678");
    }
}
