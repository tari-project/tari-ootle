//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! https://ethereum.github.io/execution-apis/api/methods/eth_sendTransaction

use tari_bor::BorError;
use tari_ootle_transaction::TransactionSealSignature;
use tari_ootle_wallet_sdk::OotleAddress;
use tari_template_lib_types::{
    bytes::Bytes,
    crypto::{RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes},
    Amount,
    FunctionName,
};

/// The raw transaction bytes. Encoded as CBOR. For human-readable formats (e.g. JSON) the base64 encoding of the CBOR
/// bytes is used.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[serde(transparent)]
pub struct SendRawTransactionRequest(#[serde(with = "ootle_serde::base64")] Box<[u8]>);

impl SendRawTransactionRequest {
    pub fn new(bytes: Box<[u8]>) -> Self {
        Self(bytes)
    }

    pub fn decode(&self) -> Result<Signed<EthStyleTransaction>, BorError> {
        tari_bor::decode(&self.0)
    }

    pub fn encode(tx: &Signed<EthStyleTransaction>) -> Result<Self, BorError> {
        let bytes = tari_bor::encode(tx)?;
        Ok(Self(bytes.into_boxed_slice()))
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct Signed<T> {
    tx: T,
    signature: TransactionSealSignature,
}

impl<T> Signed<T> {
    pub fn new(inner: T, signature: TransactionSealSignature) -> Self {
        Self { tx: inner, signature }
    }

    pub fn tx(&self) -> &T {
        &self.tx
    }

    pub fn signature(&self) -> &TransactionSealSignature {
        &self.signature
    }

    pub fn into_parts(self) -> (T, TransactionSealSignature) {
        (self.tx, self.signature)
    }
}

#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct EthStyleTransaction {
    /// EIP-155: Simple replay attack protection
    // #[cfg_attr(feature = "serde", serde(with = "alloy_serde::quantity"))]
    pub network: u64,
    // /// A scalar value equal to the number of transactions sent by the sender; formally Tn.
    // #[cfg_attr(feature = "serde", serde(with = "alloy_serde::quantity"))]
    // pub nonce: u64,
    /// A scalar value equal to the maximum
    /// amount of gas that should be used in executing
    /// this transaction. This is paid up-front, before any
    /// computation is done and may not be increased
    /// later; formally Tg.
    // #[cfg_attr(
    //     feature = "serde",
    //     serde(with = "alloy_serde::quantity", rename = "gas", alias = "gasLimit")
    // )]
    pub gas_limit: u64,
    // /// A scalar value equal to the maximum total fee per unit of gas
    // /// the sender is willing to pay. The actual fee paid per gas is
    // /// the minimum of this and `base_fee + max_priority_fee_per_gas`.
    // ///
    // /// As ethereum circulation is around 120mil eth as of 2022 that is around
    // /// 120000000000000000000000000 wei we are safe to use u128 as its max number is:
    // /// 340282366920938463463374607431768211455
    // ///
    // /// This is also known as `GasFeeCap`
    // // #[cfg_attr(feature = "serde", serde(with = "alloy_serde::quantity"))]
    // pub max_fee_per_gas: u128,
    // /// Max Priority fee that transaction is paying
    // ///
    // /// As ethereum circulation is around 120mil eth as of 2022 that is around
    // /// 120000000000000000000000000 wei we are safe to use u128 as its max number is:
    // /// 340282366920938463463374607431768211455
    // ///
    // /// This is also known as `GasTipCap`
    // // #[cfg_attr(feature = "serde", serde(with = "alloy_serde::quantity"))]
    // pub max_priority_fee_per_gas: u128,
    /// The 160-bit address of the message call’s recipient.
    pub to: OotleAddress,
    /// A scalar value equal to the number of Wei to
    /// be transferred to the message call’s recipient or,
    /// in the case of contract creation, as an endowment
    /// to the newly created account; formally Tv.
    pub value: Amount,
    // /// The accessList specifies a list of addresses and storage keys;
    // /// these addresses and storage keys are added into the `accessed_addresses`
    // /// and `accessed_storage_keys` global sets (introduced in EIP-2929).
    // /// A gas cost is charged, though at a discount relative to the cost of
    // /// accessing outside the list.
    // pub access_list: AccessList,
    /// Authorizations are used to temporarily set the code of its signer to
    /// the code referenced by `address`. These also include a `chain_id` (which
    /// can be set to zero and not evaluated) as well as an optional `nonce`.
    pub authorization_list: Vec<SignedAuthorization>,
    /// An unlimited size byte array specifying the
    /// input data of the message call, formally Td.
    pub call_data: CallData,
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct CallData {
    pub method: FunctionName,
    pub args: Vec<Bytes>,
}

#[derive(Debug, Clone, Eq, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct SignedAuthorization {
    /// Inner authorization.
    // #[cfg_attr(feature = "serde", serde(flatten))]
    // inner: Authorization,
    public_key: RistrettoPublicKeyBytes,
    /// Signature parity value. We allow any [`U8`] here, however, the only valid values are `0`
    /// and `1` and anything else will result in error during recovery.
    // #[cfg_attr(feature = "serde", serde(rename = "yParity", alias = "v"))]
    // y_parity: u8,
    /// Signature `r` value.
    r: RistrettoPublicKeyBytes,
    /// Signature `s` value.
    s: Scalar32Bytes,
}

impl SignedAuthorization {
    pub fn new(
        // inner: Authorization,
        public_key: RistrettoPublicKeyBytes,
        r: RistrettoPublicKeyBytes,
        s: Scalar32Bytes,
    ) -> Self {
        Self {
            // inner,
            public_key,
            r,
            s,
        }
    }

    pub fn public_key(&self) -> &RistrettoPublicKeyBytes {
        &self.public_key
    }

    pub fn r(&self) -> &RistrettoPublicKeyBytes {
        &self.r
    }

    pub fn s(&self) -> &Scalar32Bytes {
        &self.s
    }

    pub fn to_schnorr_signature_bytes(&self) -> SchnorrSignatureBytes {
        SchnorrSignatureBytes::new(self.r, self.s)
    }
}

// #[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
// #[serde(rename_all = "camelCase")]
// pub struct Authorization {
//     /// The chain ID of the authorization.
//     pub chain_id: U256,
//     /// The address of the authorization.
//     pub address: OotleAddress,
//     /// The nonce for the authorization.
//     // #[cfg_attr(feature = "serde", serde(with = "quantity"))]
//     pub nonce: u64,
// }
