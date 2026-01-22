//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_address::RistrettoOotleAddress;
use tari_ootle_transaction::UnsignedTransaction;
use tari_ootle_wallet_crypto::{memo::Memo, StealthInputWitness};
use tari_template_lib::{
    models::{ComponentAddress, VaultId},
    prelude::Amount,
};

use crate::{
    apis::stealth_transfer::PayTo,
    models::{InputSpendData, StealthUtxoSpendKeyId, WalletPublicKey},
};

pub struct StealthTransferOutput {
    pub transaction: UnsignedTransaction,
    pub fee_inputs: InputsToSpend,
    pub transfer_inputs: InputsToSpend,
    pub utxo_spend_keys: Vec<StealthUtxoSpendKeyId>,
    pub additional_signer: Option<WalletPublicKey>,
    pub main_signer: WalletPublicKey,
}

#[derive(Debug)]
pub struct UnblindedInputToSpend {
    pub witness: StealthInputWitness,
}

impl UnblindedInputToSpend {
    pub fn value(&self) -> u64 {
        self.witness.mask_and_value.value
    }
}

#[derive(Debug, Clone)]
pub struct StealthOutputToCreate<'a> {
    pub owner_address: RistrettoOotleAddress,
    pub pay_to: PayTo,
    pub amount: u64,
    pub memo: Option<&'a Memo>,
}

#[derive(Debug)]
pub struct InputsToSpend {
    pub inputs: Vec<InputSpendData>,
    pub revealed: Amount,
}

impl InputsToSpend {
    pub fn inputs_iter(&self) -> impl Iterator<Item = &InputSpendData> + '_ {
        self.inputs.iter()
    }

    pub fn total_amount(&self) -> Amount {
        self.total_stealth_input_amount() + self.revealed
    }

    pub fn total_stealth_input_amount(&self) -> Amount {
        self.inputs.iter().map(|i| Amount::from(i.value)).sum()
    }
}

pub struct AccountDetails {
    pub address: ComponentAddress,
    pub vaults: Vec<VaultId>,
    pub exists: bool,
}
