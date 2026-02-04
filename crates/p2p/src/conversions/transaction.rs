//  Copyright 2023, The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use std::convert::{TryFrom, TryInto};

use anyhow::anyhow;
use tari_ootle_transaction::Transaction;

use crate::{
    NewTransactionMessage,
    encoding::{decode_from_slice, encode_to_vec},
    proto::{self},
};

// -------------------------------- NewTransactionMessage -------------------------------- //
impl From<NewTransactionMessage> for proto::transaction::NewTransactionMessage {
    fn from(msg: NewTransactionMessage) -> Self {
        Self {
            transaction: Some((&msg.transaction).into()),
        }
    }
}

impl TryFrom<proto::transaction::NewTransactionMessage> for NewTransactionMessage {
    type Error = anyhow::Error;

    fn try_from(value: proto::transaction::NewTransactionMessage) -> Result<Self, Self::Error> {
        Ok(NewTransactionMessage {
            transaction: value
                .transaction
                .ok_or_else(|| anyhow!("Transaction not provided"))?
                .try_into()?,
        })
    }
}

//---------------------------------- Transaction --------------------------------------------//
impl TryFrom<proto::transaction::Transaction> for Transaction {
    type Error = anyhow::Error;

    fn try_from(transaction: proto::transaction::Transaction) -> Result<Self, Self::Error> {
        decode_from_slice(&transaction.bor_encoded)
    }
}

impl From<&Transaction> for proto::transaction::Transaction {
    fn from(transaction: &Transaction) -> Self {
        proto::transaction::Transaction {
            // TODO: no panic
            bor_encoded: encode_to_vec(transaction).expect("Failed to encode transaction"),
        }
    }
}
