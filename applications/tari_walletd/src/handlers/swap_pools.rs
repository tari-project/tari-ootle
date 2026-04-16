//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use log::warn;
use tari_engine_types::{component::ComponentHeader, indexed_value::IndexedWellKnownTypes, substate::SubstateId};
use tari_ootle_wallet_sdk::network::WalletNetworkInterface;
use tari_ootle_walletd_client::{
    permissions::JrpcPermission,
    types::{
        SwapPoolGetExchangeRateRequest,
        SwapPoolGetExchangeRateResponse,
        SwapPoolInfo,
        SwapPoolsListRequest,
        SwapPoolsListResponse,
    },
};
use tari_template_builtin::LIQUIDITY_POOL_TEMPLATE_ADDRESS;
use tari_template_lib_types::{Amount, ComponentAddress, ResourceAddress, constants::TARI_TOKEN};

use crate::handlers::HandlerContext;

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::swap_pools";

/// Calculate the input amount of a non-TARI token needed for a constant-product swap
/// to yield at least `desired_output` of TARI. Returns `None` if the pool lacks liquidity.
///
/// Includes a 5% slippage margin and uses ceiling division to avoid integer truncation issues.
fn calculate_swap_input(desired_output: Amount, reserve_in: Amount, reserve_out: Amount) -> Option<Amount> {
    if reserve_out <= desired_output {
        return None;
    }
    // ceil(desired_output * reserve_in * 105 / ((reserve_out - desired_output) * 100))
    let numerator = desired_output
        .to_u128()
        .checked_mul(reserve_in.to_u128())?
        .checked_mul(105)?;
    let denominator = reserve_out.checked_sub(desired_output)?.to_u128().checked_mul(100)?;
    // Ceiling division: (a + b - 1) / b
    let result = numerator.checked_add(denominator - 1)? / denominator;
    Some(Amount::new(result))
}

/// Given the pool vault balances, determine which side is TARI and calculate the
/// required swap input amount for the given `desired_tari_output`.
fn calculate_swap_input_for_pool(
    resource_a: &ResourceAddress,
    balance_a: Amount,
    resource_b: &ResourceAddress,
    balance_b: Amount,
    desired_tari_output: Amount,
) -> Result<Amount, anyhow::Error> {
    let (reserve_in, reserve_out) = if *resource_a == TARI_TOKEN {
        // resource_b is non-TARI (input), resource_a is TARI (output)
        (balance_b, balance_a)
    } else if *resource_b == TARI_TOKEN {
        // resource_a is non-TARI (input), resource_b is TARI (output)
        (balance_a, balance_b)
    } else {
        return Err(anyhow::anyhow!("Neither pool resource is TARI"));
    };

    calculate_swap_input(desired_tari_output, reserve_in, reserve_out)
        .ok_or_else(|| anyhow::anyhow!("Pool does not have enough TARI liquidity for the desired output"))
}

/// Fetches vault balances for a validated liquidity pool component.
async fn get_pool_vault_info(
    context: &HandlerContext,
    component: &ComponentHeader,
) -> Result<
    (
        tari_template_lib_types::ResourceAddress,
        tari_template_lib_types::Amount,
        tari_template_lib_types::ResourceAddress,
        tari_template_lib_types::Amount,
    ),
    anyhow::Error,
> {
    let sdk = context.wallet_sdk().clone();

    let indexed = IndexedWellKnownTypes::from_value(&component.body.state)?;
    let vault_ids = indexed.vault_ids();

    if vault_ids.len() < 2 {
        return Err(anyhow::anyhow!(
            "Expected at least 2 vaults in the liquidity pool component, found {}",
            vault_ids.len()
        ));
    }

    let vault_substate_ids: Vec<SubstateId> = vault_ids.iter().take(2).map(|id| SubstateId::Vault(*id)).collect();

    let vault_substates = sdk
        .get_network_interface()
        .get_substates(vault_substate_ids.clone())
        .await?;

    let vault_a_substate = vault_substates
        .get(&vault_substate_ids[0])
        .ok_or_else(|| anyhow::anyhow!("Vault A substate not found"))?;
    let vault_b_substate = vault_substates
        .get(&vault_substate_ids[1])
        .ok_or_else(|| anyhow::anyhow!("Vault B substate not found"))?;

    let vault_a = vault_a_substate
        .substate_value()
        .vault()
        .ok_or_else(|| anyhow::anyhow!("Vault A substate is not a vault"))?;
    let vault_b = vault_b_substate
        .substate_value()
        .vault()
        .ok_or_else(|| anyhow::anyhow!("Vault B substate is not a vault"))?;

    Ok((
        *vault_a.resource_address(),
        vault_a.balance(),
        *vault_b.resource_address(),
        vault_b.balance(),
    ))
}

pub async fn handle_get_exchange_rate(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: SwapPoolGetExchangeRateRequest,
) -> Result<SwapPoolGetExchangeRateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.check_auth(token, &[JrpcPermission::SubstatesRead])?;

    let result = sdk
        .get_network_interface()
        .query_substate(&SubstateId::Component(req.pool_address), None, false)
        .await?;

    let component = result
        .substate
        .into_component()
        .ok_or_else(|| anyhow::anyhow!("Substate is not a component"))?;

    if component.template_address != LIQUIDITY_POOL_TEMPLATE_ADDRESS {
        return Err(anyhow::anyhow!(
            "Component is not a liquidity pool (template: {})",
            component.template_address
        ));
    }

    let (resource_a, balance_a, resource_b, balance_b) = get_pool_vault_info(context, &component).await?;

    let swap_input_amount = match req.desired_tari_output {
        Some(desired) => Some(calculate_swap_input_for_pool(
            &resource_a,
            balance_a,
            &resource_b,
            balance_b,
            desired,
        )?),
        None => None,
    };

    Ok(SwapPoolGetExchangeRateResponse {
        resource_a,
        balance_a,
        resource_b,
        balance_b,
        swap_input_amount,
    })
}

// TODO: Resource pair filtering is done in-memory after fetching all watched pool substates.
//       This is inefficient — ideally the indexer should support filtering watched substates by
//       resource pair so we don't need to fetch and inspect every pool's vaults.
pub async fn handle_list(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: SwapPoolsListRequest,
) -> Result<SwapPoolsListResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.check_auth(token, &[JrpcPermission::SubstatesRead])?;

    let limit = req.limit.unwrap_or(10).min(50) as usize;
    let offset = req.offset.unwrap_or(0);

    // When filtering by resource pair, we need to over-fetch because not all pools will match.
    // Fetch up to 50 pools and filter down to the requested limit.
    let fetch_limit = if req.resource_pair.is_some() {
        50u64
    } else {
        limit as u64
    };

    let watched = sdk
        .get_network_interface()
        .list_watched_substates(Some(LIQUIDITY_POOL_TEMPLATE_ADDRESS), Some(fetch_limit), Some(offset))
        .await?;

    let mut pools = Vec::with_capacity(limit);
    for item in watched {
        if pools.len() >= limit {
            break;
        }

        let pool_address = match item.component_address {
            SubstateId::Component(addr) => addr,
            other => {
                warn!(target: LOG_TARGET, "Unexpected substate type in watched substates: {other}");
                continue;
            },
        };

        match fetch_pool_info(context, pool_address).await {
            Ok(info) => {
                // If a resource pair filter is set, only include pools that contain both resources
                if let Some((ref r1, ref r2)) = req.resource_pair {
                    let has_pair = (info.resource_a == *r1 && info.resource_b == *r2) ||
                        (info.resource_a == *r2 && info.resource_b == *r1);
                    if !has_pair {
                        continue;
                    }
                }
                pools.push(info);
            },
            Err(e) => {
                warn!(target: LOG_TARGET, "Failed to fetch pool info for {pool_address}: {e}");
            },
        }
    }

    Ok(SwapPoolsListResponse { pools })
}

async fn fetch_pool_info(
    context: &HandlerContext,
    pool_address: ComponentAddress,
) -> Result<SwapPoolInfo, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();

    let result = sdk
        .get_network_interface()
        .query_substate(&SubstateId::Component(pool_address), None, false)
        .await?;

    let component = result
        .substate
        .into_component()
        .ok_or_else(|| anyhow::anyhow!("Substate is not a component"))?;

    let (resource_a, balance_a, resource_b, balance_b) = get_pool_vault_info(context, &component).await?;

    Ok(SwapPoolInfo {
        pool_address,
        resource_a,
        balance_a,
        resource_b,
        balance_b,
    })
}
