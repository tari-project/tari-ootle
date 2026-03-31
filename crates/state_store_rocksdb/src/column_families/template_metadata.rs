//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::published_template::TemplateMetadata;
use tari_template_lib_types::TemplateAddress;

use crate::{
    codecs::{DefaultCodec, FixedBytesCodec32},
    column_families::cf_names,
    traits::Cf,
};

/// Column family that maps `TemplateAddress` → `TemplateMetadata`.
///
/// Written by the validator node at block-commit time whenever a template substate is committed.
/// Read by the `sync_state` handler when `TEMPLATE_METADATA` is set in the value-filter flags,
/// allowing indexers to discover template catalogues without downloading full WASM binaries.
pub struct TemplateMetadataCf;

impl Cf for TemplateMetadataCf {
    type Key = TemplateAddress;
    type KeyCodec = FixedBytesCodec32;
    type Prefix = ();
    type Value = TemplateMetadata;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::TEMPLATE_METADATA
    }
}
