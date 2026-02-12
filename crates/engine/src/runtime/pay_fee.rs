//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::args::WorkspaceOffsetId;

pub enum PayFee {
    FromBucket { bucket: WorkspaceOffsetId },
}
