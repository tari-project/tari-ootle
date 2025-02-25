// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use log::warn;
use tari_common::configuration::Network;
use tari_transaction::Transaction;

use crate::{transaction_validators::TransactionValidationError, validator::Validator};

const LOG_TARGET: &str = "tari::dan::mempool::validators::network";

#[derive(Debug)]
pub struct TransactionNetworkValidator {
    network: Network,
}

impl TransactionNetworkValidator {
    pub fn new(network: Network) -> Self {
        Self { network }
    }
}

impl Validator<Transaction> for TransactionNetworkValidator {
    type Context = ();
    type Error = TransactionValidationError;

    fn validate(&self, _context: &Self::Context, input: &Transaction) -> Result<(), Self::Error> {
        match input {
            Transaction::V1(tx) => {
                let tx_network = Network::try_from(tx.network())
                    .map_err(|error| Self::Error::UnknownNetwork(tx.network(), error))?;
                if tx_network == self.network {
                    Ok(())
                } else {
                    warn!(target: LOG_TARGET, "TransactionNetworkValidator - FAIL: mismatching networks: TX: {} != Current: {}", tx_network, self.network);
                    Err(Self::Error::NetworkMismatch {
                        actual: self.network,
                        expected: tx_network,
                    })
                }
            },
        }
    }
}

#[cfg(test)]
mod tests {
    use indexmap::IndexSet;
    use tari_common::configuration::Network;
    use tari_common_types::types::{PublicKey, Signature};
    use tari_transaction::{
        Transaction,
        TransactionSealSignature,
        TransactionSignature,
        UnsealedTransactionV1,
        UnsignedTransactionV1,
    };

    use crate::{
        transaction_validators::{TransactionNetworkValidator, TransactionValidationError},
        validator::Validator,
    };

    fn tx(network_byte: u8) -> Transaction {
        Transaction::new(
            UnsealedTransactionV1::new(
                UnsignedTransactionV1::new(network_byte, vec![], vec![], IndexSet::new(), None, None),
                vec![TransactionSignature::new(PublicKey::default(), Signature::default())],
            ),
            TransactionSealSignature::new(PublicKey::default(), Signature::default()),
        )
    }

    #[test]
    fn unknown_network() {
        let network_byte = 9u8;
        let validator = TransactionNetworkValidator::new(Network::LocalNet);
        let tx = tx(network_byte);
        let result = validator.validate(&(), &tx);
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            TransactionValidationError::UnknownNetwork(_, _)
        ));
    }

    #[test]
    fn network_mismatch() {
        let network_byte = Network::MainNet.as_byte();
        let validator = TransactionNetworkValidator::new(Network::LocalNet);
        let tx = tx(network_byte);
        let result = validator.validate(&(), &tx);
        assert!(result.is_err());
        assert!(matches!(
            result.err().unwrap(),
            TransactionValidationError::NetworkMismatch { actual: _, expected: _ },
        ));
    }

    #[test]
    fn network_ok() {
        let network_byte = Network::LocalNet.as_byte();
        let validator = TransactionNetworkValidator::new(Network::LocalNet);
        let tx = tx(network_byte);
        let result = validator.validate(&(), &tx);
        assert!(result.is_ok());
    }
}
