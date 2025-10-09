//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use bytes::BufMut;

pub trait Encoder {
    type Item;
    fn encode_into(&self, msg: &Self::Item, buf: &mut impl BufMut) -> anyhow::Result<()>;
}
