//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::published_template::{PublishedTemplateAddress, PublishedTemplateMetadata};

use crate::{
    codecs::{DefaultCodec, FixedBytesCodec32, KeyPrefix},
    column_families::cf_names,
    prefixed,
    traits::Cf,
};

prefixed!(TemplateMetadataPrefix, KeyPrefix::TemplateMetadata);
/// Column family that maps `TemplateAddress` → `TemplateMetadata`.
///
/// Written by the validator node at block-commit time whenever a template substate is committed.
/// Read by the `sync_state` handler when `TEMPLATE_METADATA` is set in the value-filter flags,
/// allowing indexers to discover template catalogues without downloading full WASM binaries.
pub struct TemplateMetadataCf;

impl Cf for TemplateMetadataCf {
    type Key = PublishedTemplateAddress;
    type KeyCodec = FixedBytesCodec32;
    type Prefix = TemplateMetadataPrefix;
    type Value = PublishedTemplateMetadata;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::TEMPLATE_METADATA
    }
}
