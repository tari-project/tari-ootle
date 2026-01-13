//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    pin::Pin,
    task::{Context, Poll},
};

use bytes::{BufMut, Bytes, BytesMut};
use futures::Stream;
use log::*;

#[derive(Debug, thiserror::Error)]
pub enum ProtobufStreamError {
    #[error("Reqwest error: {0}")]
    ReqwestError(#[from] reqwest::Error),
    #[error("Prost decode error: {0}")]
    DecodeError(#[from] prost::DecodeError),
    #[error("Message size {len} exceeds maximum allowed size of {max} bytes")]
    MessageSizeExceeded { len: usize, max: usize },
}

pub struct ProtobufStream<TMsg> {
    bytes_stream: Pin<Box<dyn Stream<Item = Result<Bytes, reqwest::Error>> + Send>>,
    buf: BytesMut,
    max_message_size: usize,
    _marker: std::marker::PhantomData<TMsg>,
}

impl<TMsg> ProtobufStream<TMsg> {
    pub fn new(bytes_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static) -> Self {
        Self::with_max_message_size(bytes_stream, 16 * 1024 * 1024) // Default to 16 MiB max message size
    }

    pub fn with_max_message_size(
        bytes_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Send + 'static,
        max_message_size: usize,
    ) -> Self {
        Self {
            bytes_stream: Box::pin(bytes_stream),
            buf: BytesMut::with_capacity(max_message_size),
            max_message_size,
            _marker: std::marker::PhantomData,
        }
    }
}

impl<TMsg: prost::Message + Default + Unpin> Stream for ProtobufStream<TMsg> {
    type Item = Result<TMsg, ProtobufStreamError>;

    fn poll_next(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        let this = self.get_mut();
        loop {
            match this.bytes_stream.as_mut().poll_next(cx) {
                Poll::Ready(Some(Ok(bytes))) => {
                    this.buf.put(bytes);

                    // Decode the length delimiter without advancing the buffer
                    // so that we can check if we have enough bytes or need to buffer more
                    let tmp_slice = &this.buf[..];
                    // A length-delimited varint is complete once a byte with MSB 0 is seen (max 10 bytes for u64).
                    if tmp_slice.len() < 10 && tmp_slice.iter().take(10).all(|byte| byte & 0x80 != 0) {
                        // Need more bytes to finish reading the delimiter.
                        continue;
                    }
                    let len = prost::decode_length_delimiter(tmp_slice)?;
                    if len > this.max_message_size {
                        return Poll::Ready(Some(Err(ProtobufStreamError::MessageSizeExceeded {
                            len,
                            max: this.max_message_size,
                        })));
                    }

                    let len_delim_len = prost::length_delimiter_len(len);
                    if this.buf.len() < len + len_delim_len {
                        // Continue buffering
                        trace!(
                            "Buffering: have {} bytes, need {} bytes (including {} bytes for length delimiter)",
                            this.buf.len(),
                            len + len_delim_len,
                            len_delim_len
                        );
                        continue;
                    }

                    let msg = TMsg::decode_length_delimited(&mut this.buf)?;
                    break Poll::Ready(Some(Ok(msg)));
                },
                Poll::Ready(Some(Err(e))) => {
                    break Poll::Ready(Some(Err(e.into())));
                },
                Poll::Ready(None) => break Poll::Ready(None),
                Poll::Pending => break Poll::Pending,
            }
        }
    }
}
