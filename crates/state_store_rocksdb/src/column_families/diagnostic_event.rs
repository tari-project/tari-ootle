//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_common_types::diagnostics::DiagnosticEvent;

use crate::{
    codecs::{DefaultCodec, KeyPrefix, NumberCodec},
    column_families::cf_names,
    prefixed,
    traits::Cf,
};

prefixed!(DiagnosticEventPrefix, KeyPrefix::DiagnosticEvents);

/// Key = a monotonically increasing sequence number, big-endian encoded so that iteration order is
/// insertion order. The sequence doubles as the event's public id and as the pagination cursor.
pub struct DiagnosticEventCf;

impl Cf for DiagnosticEventCf {
    type Key = u64;
    type KeyCodec = NumberCodec<u64>;
    type Prefix = DiagnosticEventPrefix;
    type Value = DiagnosticEvent;
    type ValueCodec = DefaultCodec<Self::Value>;

    fn name() -> &'static str {
        cf_names::DIAGNOSTICS
    }
}
