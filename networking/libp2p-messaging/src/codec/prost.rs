//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, marker::PhantomData};

// Re-export prost public types
pub use ::prost::Message;
use async_trait::async_trait;
use libp2p::futures::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};

use crate::codec::Codec;

const MAX_MESSAGE_SIZE: usize = 4 * 1024 * 1024;
const INITIAL_READ_CAPACITY: usize = 64 * 1024;

pub struct ProstCodec<TMsg>(PhantomData<TMsg>);

impl<TMsg> Default for ProstCodec<TMsg> {
    fn default() -> Self {
        Self(PhantomData)
    }
}

#[async_trait]
impl<TMsg> Codec for ProstCodec<TMsg>
where TMsg: prost::Message + fmt::Debug + Default
{
    type Message = TMsg;

    async fn decode_from<R>(&self, reader: &mut R) -> std::io::Result<(usize, Self::Message)>
    where R: AsyncRead + Unpin + Send {
        let mut len_buf = [0u8; 4];
        reader.read_exact(&mut len_buf).await?;
        let len = u32::from_be_bytes(len_buf) as usize;
        if len > MAX_MESSAGE_SIZE {
            return Err(std::io::Error::other("message too large"));
        }
        // The buffer grows as bytes arrive: each step at most doubles what has been received and never exceeds the
        // declared length.
        let mut buf = Vec::new();
        while buf.len() < len {
            let start = buf.len();
            let end = len.min(start.saturating_mul(2).max(INITIAL_READ_CAPACITY));
            buf.reserve_exact(end - start);
            buf.resize(end, 0);
            reader.read_exact(&mut buf[start..]).await?;
        }
        let mut slice = &buf[..];
        let message = prost::Message::decode(&mut slice).map_err(std::io::Error::other)?;

        if !slice.is_empty() {
            return Err(std::io::Error::other("bytes remaining on buffer"));
        }
        Ok((len, message))
    }

    async fn encode_to<W>(&self, writer: &mut W, message: Self::Message) -> std::io::Result<()>
    where W: AsyncWrite + Unpin + Send {
        let mut buf = Vec::new();
        message.encode(&mut buf).map_err(std::io::Error::other)?;
        let len = buf.len();
        if len > MAX_MESSAGE_SIZE {
            return Err(std::io::Error::other("message too large"));
        }
        writer.write_all(&(len as u32).to_be_bytes()).await?;
        writer.write_all(&buf).await?;
        Ok(())
    }
}

impl<TMsg> Clone for ProstCodec<TMsg> {
    fn clone(&self) -> Self {
        Self(PhantomData)
    }
}

impl<TMsg> fmt::Debug for ProstCodec<TMsg> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("ProstCodec").finish()
    }
}

#[cfg(test)]
mod tests {
    use libp2p::futures::executor::block_on;

    use super::*;

    #[test]
    fn round_trips_a_message() {
        let codec = ProstCodec::<Vec<u8>>::default();
        let message = vec![7u8; 100_000];
        let mut wire = Vec::new();
        block_on(codec.encode_to(&mut wire, message.clone())).unwrap();

        let (_, decoded) = block_on(codec.decode_from(&mut wire.as_slice())).unwrap();
        assert_eq!(decoded, message);
    }

    #[test]
    fn truncated_message_is_an_unexpected_eof() {
        let codec = ProstCodec::<Vec<u8>>::default();
        let mut wire = Vec::new();
        block_on(codec.encode_to(&mut wire, vec![7u8; 1000])).unwrap();
        wire.truncate(500);

        let err = block_on(codec.decode_from(&mut wire.as_slice())).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::UnexpectedEof);
    }
}
