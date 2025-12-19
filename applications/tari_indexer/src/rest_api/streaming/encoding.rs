//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{io::Write, str::FromStr};

use bytes::{BufMut, Bytes};
use log::*;
use mediatype::MediaType;
use prost::Message;
use tari_indexer_client::protobuf::UtxoUpdatePayload;

use crate::rest_api::encoder::Encoder;

const LOG_TARGET: &str = "tari::indexer::rest_api::streaming::encoding";

pub struct ProtobufEncoder;

impl Encoder for ProtobufEncoder {
    type Item = UtxoUpdatePayload;

    fn encode_into(&self, msg: &Self::Item, buf: &mut impl BufMut) -> anyhow::Result<()> {
        // Length-delimited protobuf
        if log_enabled!(Level::Trace) {
            let len = msg.encoded_len();
            trace!(target: LOG_TARGET, "🚧 Encoding protobuf message of length: {}", len);
        }
        let len = msg.encoded_len();
        debug!(target: LOG_TARGET, "🚧 Encoding protobuf message of length: {}", len);
        msg.encode_length_delimited(buf)?;
        Ok(())
    }
}

pub struct JsonEncoder;

impl Encoder for JsonEncoder {
    type Item = UtxoUpdatePayload;

    fn encode_into(&self, msg: &Self::Item, buf: &mut impl BufMut) -> anyhow::Result<()> {
        let json = serde_json::to_vec(msg)?;
        buf.put(Bytes::from(json));
        // Line delimited
        buf.writer().write_all(b"\n")?;
        Ok(())
    }
}

pub enum MimeTypeEncoder {
    Protobuf(ProtobufEncoder),
    Json(JsonEncoder),
}

impl MimeTypeEncoder {
    pub fn protobuf() -> Self {
        MimeTypeEncoder::Protobuf(ProtobufEncoder)
    }
}

impl Encoder for MimeTypeEncoder {
    type Item = UtxoUpdatePayload;

    fn encode_into(&self, msg: &Self::Item, buf: &mut impl BufMut) -> anyhow::Result<()> {
        match self {
            MimeTypeEncoder::Protobuf(encoder) => encoder.encode_into(msg, buf),
            MimeTypeEncoder::Json(encoder) => encoder.encode_into(msg, buf),
        }
    }
}

pub fn from_media_type(mime_type: &str) -> Option<MimeTypeEncoder> {
    const ACCEPTED_TYPES: &[MediaType] = &[
        MediaType::new(
            mediatype::Name::new_unchecked("application"),
            mediatype::Name::new_unchecked("x-protobuf"),
        ),
        MediaType::new(
            mediatype::Name::new_unchecked("application"),
            mediatype::Name::new_unchecked("json"),
        ),
    ];
    let media_type = headers_accept::Accept::from_str(mime_type).ok()?;
    let accepted = media_type.negotiate(ACCEPTED_TYPES)?;
    match accepted.subty.as_str() {
        "x-protobuf" => Some(MimeTypeEncoder::Protobuf(ProtobufEncoder)),
        "json" => Some(MimeTypeEncoder::Json(JsonEncoder)),
        _ => None,
    }
}
