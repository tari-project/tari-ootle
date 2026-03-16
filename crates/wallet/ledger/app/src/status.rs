//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use alloc::{borrow::Cow, vec::Vec};

use ledger_device_sdk::io::{Reply, StatusWords};
use ootle_ledger_common::OotleStatusWord;

pub enum AppStatus {
    OotleStatusWord(OotleStatusWord),
    StatusWords(StatusWords),
    StatusWithMessage {
        message: Cow<'static, str>,
        status: StatusWords,
    },
    OotleStatusWithMessages {
        messages: Vec<Cow<'static, str>>,
        status: OotleStatusWord,
    },
}

impl From<StatusWords> for AppStatus {
    fn from(status_words: StatusWords) -> Self {
        Self::StatusWords(status_words)
    }
}

impl From<OotleStatusWord> for AppStatus {
    fn from(ootle_status_word: OotleStatusWord) -> Self {
        Self::OotleStatusWord(ootle_status_word)
    }
}

impl From<AppStatus> for Reply {
    fn from(app_status_word: AppStatus) -> Self {
        match app_status_word {
            AppStatus::OotleStatusWord(status) => Reply(status.to_status()),
            AppStatus::StatusWords(status) => Reply(status as u16),
            AppStatus::StatusWithMessage { status, .. } => Reply(status as u16),
            AppStatus::OotleStatusWithMessages { status, .. } => Reply(status.to_status()),
        }
    }
}
