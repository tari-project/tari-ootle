//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use ledger_transport_hid::TransportNativeHID;

use crate::LedgerClient;

/// Native HID ledger client
pub type LedgerHidClient = LedgerClient<TransportNativeHID>;
