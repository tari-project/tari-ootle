//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use log::warn;
use tari_engine_types::substate::SubstateId;
use tari_ootle_wallet_sdk::{apis::swap_pool, network::WalletNetworkInterface};
use tari_ootle_walletd_client::{
    permissions::{Permission, ReadOnly},
    types::{
        SwapPoolGetExchangeRateRequest,
        SwapPoolGetExchangeRateResponse,
        SwapPoolInfo,
        SwapPoolsListRequest,
        SwapPoolsListResponse,
    },
};
use tari_template_builtin::LIQUIDITY_POOL_TEMPLATE_ADDRESS;
use tari_template_lib_types::ComponentAddress;

use crate::handlers::HandlerContext;

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::swap_pools";

pub async fn handle_get_exchange_rate(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: SwapPoolGetExchangeRateRequest,
) -> Result<SwapPoolGetExchangeRateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.authorize(token, &[Permission::SwapPools(ReadOnly::Read)])?;

    let substate_api = sdk.substate_api();
    let reserves = swap_pool::get_pool_reserves(&substate_api, req.pool_address).await?;

    let swap_input_amount = match req.desired_tari_output {
        Some(desired) => {
            let (_non_tari_resource, reserve_in, reserve_out) = reserves
                .tari_swap_reserves()
                .ok_or_else(|| anyhow::anyhow!("Neither pool resource is TARI"))?;
            Some(
                swap_pool::calculate_swap_input(desired, reserve_in, reserve_out).ok_or_else(|| {
                    anyhow::anyhow!("Pool does not have enough TARI liquidity for the desired output")
                })?,
            )
        },
        None => None,
    };

    Ok(SwapPoolGetExchangeRateResponse {
        resource_a: reserves.resource_a,
        balance_a: reserves.balance_a,
        resource_b: reserves.resource_b,
        balance_b: reserves.balance_b,
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
    context.authorize(token, &[Permission::SwapPools(ReadOnly::Read)])?;

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

    let substate_api = sdk.substate_api();
    let reserves = swap_pool::get_pool_reserves(&substate_api, pool_address).await?;

    Ok(SwapPoolInfo {
        pool_address,
        resource_a: reserves.resource_a,
        balance_a: reserves.balance_a,
        resource_b: reserves.resource_b,
        balance_b: reserves.balance_b,
    })
}
