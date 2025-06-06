//    Copyright 2023 The Tari Project
//    SPDX-License-Identifier: BSD-3-Clause

use std::fmt::{Display, Formatter};

use serde::Serialize;
use tari_transaction::Transaction;

#[derive(Debug, Clone, Serialize)]
pub enum TariMessage {
    // Mempool
    NewTransaction(Box<NewTransactionMessage>),
}

impl TariMessage {
    pub fn as_type_str(&self) -> &'static str {
        match self {
            Self::NewTransaction(_) => "NewTransaction",
        }
    }

    pub fn get_message_tag(&self) -> String {
        match self {
            Self::NewTransaction(msg) => format!("tx_{}", msg.transaction.calculate_id()),
        }
    }
}

impl From<NewTransactionMessage> for TariMessage {
    fn from(value: NewTransactionMessage) -> Self {
        Self::NewTransaction(Box::new(value))
    }
}

impl Display for TariMessage {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::NewTransaction(msg) => write!(f, "NewTransaction({})", msg.transaction.calculate_id()),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct NewTransactionMessage {
    pub transaction: Transaction,
}
