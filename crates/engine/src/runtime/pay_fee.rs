//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::args::WorkspaceOffsetId;
use tari_template_lib::types::stealth::StealthTransferStatement;

pub enum PayFee {
    FromStealth {
        statement: StealthTransferStatement,
        input_bucket: Option<WorkspaceOffsetId>,
    },
    FromBucket {
        bucket: WorkspaceOffsetId,
    },
}
