//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::{
    indexed_value::{IndexedValueError, IndexedWellKnownTypes},
    substate::SubstateId,
};
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_template_builtin::LIQUIDITY_POOL_TEMPLATE_ADDRESS;
use tari_template_lib::types::{Amount, ComponentAddress, ResourceAddress, TemplateAddress, constants::TARI_TOKEN};

use crate::{
    apis::substate::{SubstateApiError, SubstatesApi},
    network::WalletNetworkInterface,
    storage::WalletStore,
};

/// Slippage margin (percent) applied when deriving a swap input amount, to account for the pool reserves moving
/// between the time the input is computed and the time the swap executes on-chain.
const SLIPPAGE_MARGIN_PERCENT: u128 = 5;

#[derive(Debug, thiserror::Error)]
pub enum SwapPoolApiError {
    #[error("Substate API error: {0}")]
    SubstateApi(#[from] SubstateApiError),
    #[error("Indexed value error: {0}")]
    IndexedValue(#[from] IndexedValueError),
    #[error("Substate {pool_address} is not a component")]
    NotAComponent { pool_address: ComponentAddress },
    #[error("Component {pool_address} is not a liquidity pool (template: {template_address})")]
    NotALiquidityPool {
        pool_address: ComponentAddress,
        template_address: TemplateAddress,
    },
    #[error("Expected at least 2 vaults in liquidity pool component, found {found}")]
    InsufficientVaults { found: usize },
    #[error("Vault substate {vault} not found")]
    VaultNotFound { vault: SubstateId },
    #[error("Substate {vault} is not a vault")]
    NotAVault { vault: SubstateId },
    #[error("Neither resource in pool {pool_address} is TARI")]
    NeitherResourceIsTari { pool_address: ComponentAddress },
    #[error(
        "Swap input resource {input_resource} is not the non-TARI side of pool {pool_address} (expected \
         {expected_resource})"
    )]
    InputResourceMismatch {
        pool_address: ComponentAddress,
        input_resource: ResourceAddress,
        expected_resource: ResourceAddress,
    },
    #[error("Pool {pool_address} does not have enough TARI liquidity for the desired output of {desired_output}")]
    InsufficientPoolLiquidity {
        pool_address: ComponentAddress,
        desired_output: Amount,
    },
    #[error("Desired TARI output must be positive")]
    NonPositiveDesiredOutput,
}

/// The resolved reserves of a two-resource liquidity pool.
#[derive(Debug, Clone)]
pub struct PoolReserves {
    pub resource_a: ResourceAddress,
    pub balance_a: Amount,
    pub resource_b: ResourceAddress,
    pub balance_b: Amount,
}

impl PoolReserves {
    /// Identifies the TARI side of the pool and returns `(non_tari_resource, reserve_in, reserve_out)` where
    /// `reserve_in` is the non-TARI (input) reserve and `reserve_out` is the TARI (output) reserve. Returns `None` if
    /// neither resource is TARI.
    pub fn tari_swap_reserves(&self) -> Option<(ResourceAddress, Amount, Amount)> {
        if self.resource_a == TARI_TOKEN {
            Some((self.resource_b, self.balance_b, self.balance_a))
        } else if self.resource_b == TARI_TOKEN {
            Some((self.resource_a, self.balance_a, self.balance_b))
        } else {
            None
        }
    }
}

/// Calculate the input amount of a non-TARI token needed for a constant-product swap to yield at least
/// `desired_output` of the output token. Returns `None` if the pool lacks enough output liquidity.
///
/// Includes a [`SLIPPAGE_MARGIN_PERCENT`] slippage margin and uses ceiling division to avoid integer truncation.
pub fn calculate_swap_input(desired_output: Amount, reserve_in: Amount, reserve_out: Amount) -> Option<Amount> {
    if reserve_out <= desired_output {
        return None;
    }
    // ceil(desired_output * reserve_in * (100 + margin) / ((reserve_out - desired_output) * 100))
    let numerator = desired_output
        .to_u128()
        .checked_mul(reserve_in.to_u128())?
        .checked_mul(100 + SLIPPAGE_MARGIN_PERCENT)?;
    let denominator = reserve_out.checked_sub(desired_output)?.to_u128().checked_mul(100)?;
    // Ceiling division: (a + b - 1) / b
    let result = numerator.checked_add(denominator - 1)? / denominator;
    Some(Amount::new(result))
}

/// Fetches and resolves the vault reserves of a liquidity pool component from the network.
pub async fn get_pool_reserves<TStore, TNetworkInterface>(
    substate_api: &SubstatesApi<'_, TStore, TNetworkInterface>,
    pool_address: ComponentAddress,
) -> Result<PoolReserves, SwapPoolApiError>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    let scan = substate_api
        .fetch_substate_from_network(&SubstateId::Component(pool_address), None)
        .await?;

    let component = scan
        .substate
        .component()
        .ok_or(SwapPoolApiError::NotAComponent { pool_address })?;

    if *component.template_address() != LIQUIDITY_POOL_TEMPLATE_ADDRESS {
        return Err(SwapPoolApiError::NotALiquidityPool {
            pool_address,
            template_address: *component.template_address(),
        });
    }

    let indexed = IndexedWellKnownTypes::from_value(component.state())?;
    let vault_ids = indexed.vault_ids();
    if vault_ids.len() < 2 {
        return Err(SwapPoolApiError::InsufficientVaults { found: vault_ids.len() });
    }

    let vault_substate_ids: Vec<SubstateId> = vault_ids.iter().take(2).map(|id| SubstateId::Vault(*id)).collect();
    let vault_substates = substate_api
        .get_substates_from_network(vault_substate_ids.clone())
        .await?;

    let mut reserves = Vec::with_capacity(2);
    for id in &vault_substate_ids {
        let vault = vault_substates
            .get(id)
            .ok_or_else(|| SwapPoolApiError::VaultNotFound { vault: id.clone() })?
            .substate_value()
            .vault()
            .ok_or_else(|| SwapPoolApiError::NotAVault { vault: id.clone() })?;
        reserves.push((*vault.resource_address(), vault.balance()));
    }

    let (resource_a, balance_a) = reserves[0];
    let (resource_b, balance_b) = reserves[1];
    Ok(PoolReserves {
        resource_a,
        balance_a,
        resource_b,
        balance_b,
    })
}

/// Derives the amount of `input_resource` that must be swapped through `pool_address` to yield at least
/// `desired_tari_output` of TARI, using the pool's current reserves. This is what a caller would otherwise have to
/// compute up-front via [`get_pool_reserves`] + [`calculate_swap_input`].
pub async fn derive_swap_input_for_tari_output<TStore, TNetworkInterface>(
    substate_api: &SubstatesApi<'_, TStore, TNetworkInterface>,
    pool_address: ComponentAddress,
    input_resource: ResourceAddress,
    desired_tari_output: Amount,
) -> Result<Amount, SwapPoolApiError>
where
    TStore: WalletStore,
    TNetworkInterface: WalletNetworkInterface,
    TNetworkInterface::Error: IsNotFoundError,
{
    if !desired_tari_output.is_positive() {
        return Err(SwapPoolApiError::NonPositiveDesiredOutput);
    }

    let reserves = get_pool_reserves(substate_api, pool_address).await?;
    let (non_tari_resource, reserve_in, reserve_out) = reserves
        .tari_swap_reserves()
        .ok_or(SwapPoolApiError::NeitherResourceIsTari { pool_address })?;

    if input_resource != non_tari_resource {
        return Err(SwapPoolApiError::InputResourceMismatch {
            pool_address,
            input_resource,
            expected_resource: non_tari_resource,
        });
    }

    calculate_swap_input(desired_tari_output, reserve_in, reserve_out).ok_or(
        SwapPoolApiError::InsufficientPoolLiquidity {
            pool_address,
            desired_output: desired_tari_output,
        },
    )
}

#[cfg(test)]
mod tests {
    use tari_template_lib::types::ObjectKey;

    use super::*;

    fn amt(v: u128) -> Amount {
        Amount::new(v)
    }

    /// The constant-product output for a given input, matching the on-chain pool: `Δy = y·Δx / (x + Δx)` (floored).
    fn constant_product_output(input: Amount, reserve_in: Amount, reserve_out: Amount) -> u128 {
        let dx = input.to_u128();
        dx * reserve_out.to_u128() / (reserve_in.to_u128() + dx)
    }

    fn non_tari_resource() -> ResourceAddress {
        ResourceAddress::new(ObjectKey::from_array([7u8; ObjectKey::LENGTH]))
    }

    #[test]
    fn returns_none_when_pool_cannot_cover_desired_output() {
        // reserve_out equal to or below the desired output can never yield it
        assert!(calculate_swap_input(amt(1000), amt(1000), amt(1000)).is_none());
        assert!(calculate_swap_input(amt(1500), amt(1000), amt(1000)).is_none());
    }

    #[test]
    fn derived_input_yields_at_least_desired_output() {
        let desired = amt(1500);
        let reserve_in = amt(1_000_000);
        let reserve_out = amt(2_000_000);

        let input = calculate_swap_input(desired, reserve_in, reserve_out).unwrap();
        assert!(input.is_positive());

        // The pool's own constant-product math on this input must clear the desired output (with margin to spare).
        let output = constant_product_output(input, reserve_in, reserve_out);
        assert!(
            output >= desired.to_u128(),
            "swap of {input} should yield >= {desired}, got {output}"
        );
    }

    #[test]
    fn applies_slippage_margin_over_the_exact_input() {
        // Symmetric reserves: exact input for 100 out is ceil(100*1000/900) = 112; with the 5% margin we expect more.
        let reserve_in = amt(1000);
        let reserve_out = amt(1000);
        let desired = amt(100);

        let input = calculate_swap_input(desired, reserve_in, reserve_out).unwrap();
        // ceil(100 * 1000 * 105 / (900 * 100)) = ceil(10_500_000 / 90_000) = 117
        assert_eq!(input, amt(117));
    }

    #[test]
    fn tari_swap_reserves_identifies_the_tari_side() {
        let other = non_tari_resource();

        // TARI on side A: input reserve is the non-TARI (B) balance, output reserve is the TARI (A) balance
        let reserves = PoolReserves {
            resource_a: TARI_TOKEN,
            balance_a: amt(500),
            resource_b: other,
            balance_b: amt(300),
        };
        assert_eq!(reserves.tari_swap_reserves(), Some((other, amt(300), amt(500))));

        // TARI on side B
        let reserves = PoolReserves {
            resource_a: other,
            balance_a: amt(300),
            resource_b: TARI_TOKEN,
            balance_b: amt(500),
        };
        assert_eq!(reserves.tari_swap_reserves(), Some((other, amt(300), amt(500))));
    }

    #[test]
    fn tari_swap_reserves_none_when_no_tari_side() {
        // Both keys must differ from TARI_TOKEN, which is ObjectKey([1u8; LENGTH]).
        let a = ResourceAddress::new(ObjectKey::from_array([3u8; ObjectKey::LENGTH]));
        let b = ResourceAddress::new(ObjectKey::from_array([4u8; ObjectKey::LENGTH]));
        let reserves = PoolReserves {
            resource_a: a,
            balance_a: amt(300),
            resource_b: b,
            balance_b: amt(500),
        };
        assert_eq!(reserves.tari_swap_reserves(), None);
    }
}
