//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    io,
    pin::Pin,
    task::{Context, Poll, ready},
};

use bytes::{Bytes, BytesMut};
use futures::{AsyncRead, AsyncWrite};
use tokio::io::ReadBuf;
use tokio_util::codec::{Decoder, Encoder, Framed, LengthDelimitedCodec};

/// The largest buffer a single read of the underlying stream is handed.
const MAX_READ_CHUNK: usize = 64 * 1024;

/// Size of the big-endian `u32` length header that precedes every frame.
const FRAME_HEADER_LEN: usize = 4;

/// Canonical framing
pub type CanonicalFraming<T> = Framed<BoundedReadCompat<T>, CanonicalCodec>;

/// Create a length-delimited frame around the given stream reader/writer with the given maximum frame length.
pub fn canonical<T>(stream: T, max_frame_len: usize) -> CanonicalFraming<T>
where T: AsyncRead + AsyncWrite + Unpin {
    Framed::new(BoundedReadCompat { inner: stream }, CanonicalCodec::new(max_frame_len))
}

/// A [`LengthDelimitedCodec`] (4-byte big-endian length header) whose read buffer grows only as frame bytes arrive.
///
/// [`LengthDelimitedCodec`] reserves a frame's declared length as soon as the header is decoded. This codec hands it
/// a frame only once the whole frame is buffered, so that reservation is always already satisfied and the read buffer
/// is sized by [`Framed`]'s growth as bytes are received.
#[derive(Debug)]
pub struct CanonicalCodec {
    inner: LengthDelimitedCodec,
    max_frame_len: usize,
}

impl CanonicalCodec {
    fn new(max_frame_len: usize) -> Self {
        Self {
            inner: LengthDelimitedCodec::builder()
                .length_field_length(FRAME_HEADER_LEN)
                .max_frame_length(max_frame_len)
                .new_codec(),
            max_frame_len,
        }
    }
}

impl Decoder for CanonicalCodec {
    type Error = io::Error;
    type Item = BytesMut;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<BytesMut>, io::Error> {
        let Some(header) = src.first_chunk::<FRAME_HEADER_LEN>() else {
            return Ok(None);
        };
        let frame_len = usize::try_from(u32::from_be_bytes(*header)).unwrap_or(usize::MAX);
        // An oversized length goes to the inner codec, which rejects it before reserving anything.
        if frame_len <= self.max_frame_len && src.len() < FRAME_HEADER_LEN.saturating_add(frame_len) {
            return Ok(None);
        }
        self.inner.decode(src)
    }
}

impl Encoder<Bytes> for CanonicalCodec {
    type Error = io::Error;

    fn encode(&mut self, data: Bytes, dst: &mut BytesMut) -> Result<(), io::Error> {
        self.inner.encode(data, dst)
    }
}

/// Adapts a futures-io stream to the tokio-io traits [`Framed`] requires, handing each read of the stream at most
/// [`MAX_READ_CHUNK`] bytes of buffer.
///
/// [`Framed`] offers each read all of the read buffer's spare capacity, which grows geometrically with the bytes
/// buffered. A futures-io read takes an initialised slice, so every byte offered is zero-filled first. Capping the
/// slice bounds the zero-filling each read does to one chunk, however much of a frame is already buffered.
#[derive(Debug)]
pub struct BoundedReadCompat<T> {
    inner: T,
}

impl<T> BoundedReadCompat<T> {
    /// Returns a reference to the wrapped stream.
    pub fn get_ref(&self) -> &T {
        &self.inner
    }

    /// Returns a mutable reference to the wrapped stream.
    pub fn get_mut(&mut self) -> &mut T {
        &mut self.inner
    }
}

impl<T: AsyncRead + Unpin> tokio::io::AsyncRead for BoundedReadCompat<T> {
    fn poll_read(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &mut ReadBuf<'_>) -> Poll<io::Result<()>> {
        let len = buf.remaining().min(MAX_READ_CHUNK);
        let slice = buf.initialize_unfilled_to(len);
        let n = ready!(Pin::new(&mut self.get_mut().inner).poll_read(cx, slice))?;
        buf.advance(n);
        Poll::Ready(Ok(()))
    }
}

impl<T: AsyncWrite + Unpin> tokio::io::AsyncWrite for BoundedReadCompat<T> {
    fn poll_write(self: Pin<&mut Self>, cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write(cx, buf)
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_flush(cx)
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        Pin::new(&mut self.get_mut().inner).poll_close(cx)
    }

    fn poll_write_vectored(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[io::IoSlice<'_>],
    ) -> Poll<io::Result<usize>> {
        Pin::new(&mut self.get_mut().inner).poll_write_vectored(cx, bufs)
    }
}

#[cfg(test)]
mod tests {
    use std::collections::VecDeque;

    use bytes::Bytes;
    use futures::{FutureExt, SinkExt, StreamExt};
    use tokio_util::compat::TokioAsyncReadCompatExt;

    use super::*;
    use crate::RPC_MAX_FRAME_SIZE;

    /// A futures-io stream that yields queued chunks, then stays pending, recording the largest buffer any read
    /// is handed.
    #[derive(Default)]
    struct ScriptedReader {
        chunks: VecDeque<Vec<u8>>,
        largest_read_buf: usize,
    }

    impl AsyncRead for ScriptedReader {
        fn poll_read(mut self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &mut [u8]) -> Poll<io::Result<usize>> {
            self.largest_read_buf = self.largest_read_buf.max(buf.len());
            match self.chunks.pop_front() {
                Some(chunk) => {
                    let n = chunk.len().min(buf.len());
                    buf[..n].copy_from_slice(&chunk[..n]);
                    if n < chunk.len() {
                        self.chunks.push_front(chunk[n..].to_vec());
                    }
                    Poll::Ready(Ok(n))
                },
                None => Poll::Pending,
            }
        }
    }

    impl AsyncWrite for ScriptedReader {
        fn poll_write(self: Pin<&mut Self>, _cx: &mut Context<'_>, buf: &[u8]) -> Poll<io::Result<usize>> {
            Poll::Ready(Ok(buf.len()))
        }

        fn poll_flush(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }

        fn poll_close(self: Pin<&mut Self>, _cx: &mut Context<'_>) -> Poll<io::Result<()>> {
            Poll::Ready(Ok(()))
        }
    }

    fn frame_header(len: usize) -> Vec<u8> {
        u32::try_from(len).unwrap().to_be_bytes().to_vec()
    }

    fn largest_read_buf(framed: &CanonicalFraming<ScriptedReader>) -> usize {
        framed.get_ref().get_ref().largest_read_buf
    }

    #[test]
    fn reads_of_a_declared_max_size_frame_are_handed_bounded_buffers() {
        let mut reader = ScriptedReader::default();
        reader.chunks.push_back(frame_header(RPC_MAX_FRAME_SIZE));
        let mut framed = canonical(reader, RPC_MAX_FRAME_SIZE);

        assert!(framed.next().now_or_never().is_none());
        let largest = largest_read_buf(&framed);
        assert!(
            largest <= MAX_READ_CHUNK,
            "a header-only frame handed a {largest} byte buffer to a single read"
        );

        for _ in 0..16 {
            framed.get_mut().get_mut().chunks.push_back(vec![0u8]);
            assert!(framed.next().now_or_never().is_none());
        }
        let largest = largest_read_buf(&framed);
        assert!(
            largest <= MAX_READ_CHUNK,
            "a trickled byte handed a {largest} byte buffer to a single read"
        );
    }

    #[test]
    fn read_buffer_capacity_tracks_the_bytes_received() {
        let mut reader = ScriptedReader::default();
        reader.chunks.push_back(frame_header(RPC_MAX_FRAME_SIZE));
        let mut framed = canonical(reader, RPC_MAX_FRAME_SIZE);

        assert!(framed.next().now_or_never().is_none());
        let capacity = framed.read_buffer().capacity();
        assert!(
            capacity <= MAX_READ_CHUNK,
            "a header-only frame left a {capacity} byte read buffer"
        );

        let received = 1024 * 1024;
        framed.get_mut().get_mut().chunks.push_back(vec![0u8; received]);
        assert!(framed.next().now_or_never().is_none());
        assert_eq!(framed.read_buffer().len(), 4 + received);
        let capacity = framed.read_buffer().capacity();
        assert!(
            capacity <= 2 * (4 + received) + MAX_READ_CHUNK,
            "{received} received bytes left a {capacity} byte read buffer"
        );
    }

    #[test]
    fn decodes_a_max_size_frame_delivered_in_pieces() {
        let payload = (0..RPC_MAX_FRAME_SIZE)
            .map(|i| u8::try_from(i % 251).unwrap())
            .collect::<Vec<_>>();
        let mut reader = ScriptedReader::default();
        reader.chunks.push_back(frame_header(RPC_MAX_FRAME_SIZE));
        for piece in payload.chunks(100_003) {
            reader.chunks.push_back(piece.to_vec());
        }
        let mut framed = canonical(reader, RPC_MAX_FRAME_SIZE);

        let frame = framed.next().now_or_never().unwrap().unwrap().unwrap();
        assert_eq!(frame.as_ref(), payload.as_slice());
    }

    #[test]
    fn rejects_a_frame_longer_than_the_maximum() {
        let mut reader = ScriptedReader::default();
        reader.chunks.push_back(frame_header(RPC_MAX_FRAME_SIZE + 1));
        let mut framed = canonical(reader, RPC_MAX_FRAME_SIZE);

        let err = framed.next().now_or_never().unwrap().unwrap().unwrap_err();
        assert_eq!(err.kind(), io::ErrorKind::InvalidData);
    }

    #[tokio::test]
    async fn round_trips_a_max_size_frame_over_a_stream() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let mut client = canonical(client.compat(), RPC_MAX_FRAME_SIZE);
        let mut server = canonical(server.compat(), RPC_MAX_FRAME_SIZE);
        let payload = Bytes::from(vec![7u8; RPC_MAX_FRAME_SIZE]);

        let send = tokio::spawn({
            let payload = payload.clone();
            async move { client.send(payload).await.unwrap() }
        });
        let frame = server.next().await.unwrap().unwrap();
        send.await.unwrap();
        assert_eq!(frame.freeze(), payload);
    }
}
