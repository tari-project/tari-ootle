//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use serde::{Deserialize, Serialize};
use tari_consensus_types::BlockId;

use crate::{
    codecs::{BlockIdCodec, DefaultCodec},
    traits::Cf,
};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct DiagnosticsNoVoteData {
    pub reason: Box<str>,
}

pub struct DiagnosticsNoVoteCf;

impl Cf for DiagnosticsNoVoteCf {
    type Key = BlockId;
    type KeyCodec = BlockIdCodec;
    type Value = DiagnosticsNoVoteData;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        "diagnostic_no_votes"
    }
}
