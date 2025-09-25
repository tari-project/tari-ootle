//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::ops::RangeInclusive;

use tari_crypto::ristretto::RistrettoSecretKey;
use tari_engine_types::{
    crypto::{ElgamalVerifiableBalance, PrivateOutput, ValueLookupTable},
    ConvertFromByteType,
};
use tari_ootle_wallet_crypto::WalletCryptoError;

#[derive(Debug, Clone)]
pub struct ViewableBalanceApi;

impl ViewableBalanceApi {
    pub fn try_brute_force_commitment_balances<'a, TLookup, TOutputsIter>(
        &self,
        secret_view_key: &RistrettoSecretKey,
        outputs: TOutputsIter,
        value_range: RangeInclusive<u64>,
        lookup: &mut TLookup,
    ) -> Result<Vec<Option<u64>>, ViewableBalanceApiError>
    where
        TLookup: ValueLookupTable,
        TOutputsIter: Iterator<Item = &'a PrivateOutput>,
    {
        let outputs_viewable_balance_decompressed = outputs
            .filter_map(|output| output.viewable_balance.as_ref())
            .map(ElgamalVerifiableBalance::convert_from_byte_type)
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| WalletCryptoError::InvalidArgument {
                name: "outputs",
                details: "Malformed viewable balance in output when decompressing ElgamalVerifiableBalance for brute \
                          forcing"
                    .to_string(),
            })?;

        let results = ElgamalVerifiableBalance::batched_brute_force(
            secret_view_key,
            value_range,
            lookup,
            &outputs_viewable_balance_decompressed,
        )
        .map_err(|e| ViewableBalanceApiError::ValueLookupTableError { details: e.to_string() })?;

        Ok(results)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum ViewableBalanceApiError {
    #[error(transparent)]
    WalletCryptoError(#[from] WalletCryptoError),
    #[error("ValueLookupTable error: {details}")]
    ValueLookupTableError { details: String },
}
