//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_consensus_types::DirectiveId;
use tari_ootle_storage::consensus_models::AppliedDirective;

use crate::{
    codecs::{DefaultCodec, FixedBytesCodec, KeyPrefix},
    column_families::cf_names,
    prefixed,
    traits::Cf,
};

prefixed!(AppliedDirectivesPrefix, KeyPrefix::AppliedDirectives);

pub struct AppliedDirectivesCf;

impl Cf for AppliedDirectivesCf {
    type Key = DirectiveId;
    type KeyCodec = FixedBytesCodec<{ DirectiveId::LENGTH }>;
    type Prefix = AppliedDirectivesPrefix;
    type Value = AppliedDirective;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::CHAIN_METADATA
    }
}
