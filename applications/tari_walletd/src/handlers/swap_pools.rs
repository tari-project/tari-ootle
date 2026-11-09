//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use axum_extra::headers::authorization::Bearer;
use log::warn;
use tari_engine_types::substate::SubstateId;
use tari_ootle_common_types::optional::IsNotFoundError;
use tari_ootle_wallet_sdk::{
    apis::swap_pool::{self, SwapPoolApiError},
    network::WalletNetworkInterface,
};
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
use tari_template_lib_types::{Amount, ComponentAddress};

use crate::handlers::{
    HandlerContext,
    helpers::{invalid_params, not_found},
};

const LOG_TARGET: &str = "tari::ootle::wallet_daemon::handlers::swap_pools";

pub async fn handle_get_exchange_rate(
    context: &HandlerContext,
    token: Option<&Bearer>,
    req: SwapPoolGetExchangeRateRequest,
) -> Result<SwapPoolGetExchangeRateResponse, anyhow::Error> {
    let sdk = context.wallet_sdk().clone();
    context.authorize(token, &[Permission::SwapPools(ReadOnly::Read)])?;

    if let Some(desired) = req.desired_tari_output &&
        !desired.is_positive()
    {
        return Err(invalid_params(
            "desired_tari_output",
            Some("Desired TARI output must be positive"),
        ));
    }

    let substate_api = sdk.substate_api();
    let reserves = swap_pool::get_pool_reserves(&substate_api, req.pool_address)
        .await
        .map_err(|e| match &e {
            SwapPoolApiError::SubstateApi(err) if err.is_not_found_error() => not_found(e),
            SwapPoolApiError::VaultNotFound { .. } => not_found(e),
            _ => e.into(),
        })?;

    let swap_input_amount = match req.desired_tari_output {
        Some(desired) => {
            let (_non_tari_resource, reserve_in, reserve_out) = reserves
                .tari_swap_reserves()
                .ok_or_else(|| anyhow::anyhow!("Neither pool resource is TARI"))?;
            Some(
                swap_pool::calculate_swap_input(desired, reserve_in, reserve_out)
                    .filter(Amount::is_positive)
                    .ok_or_else(|| {
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
    if limit == 0 {
        return Ok(SwapPoolsListResponse { pools: Vec::new() });
    }
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

#[cfg(test)]
mod tests {
    use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
    use tari_template_lib_types::ResourceAddress;

    use super::*;
    use crate::handlers::test_support::TestDaemon;

    #[tokio::test]
    async fn exchange_rate_rejects_non_positive_desired_output_and_list_short_circuits_zero_limit() {
        let daemon = TestDaemon::start().await;
        let pool_address = ComponentAddress::from_array([0x44; 32]);

        for desired in [Amount::zero(), Amount::from(-25i64)] {
            let err = handle_get_exchange_rate(
                &daemon.context,
                Some(&daemon.session),
                SwapPoolGetExchangeRateRequest {
                    pool_address,
                    desired_tari_output: Some(desired),
                },
            )
            .await
            .unwrap_err();
            let jrpc = err.downcast_ref::<JsonRpcError>().expect("JsonRpcError");
            assert!(matches!(jrpc.error_reason(), JsonRpcErrorReason::InvalidParams));
            assert!(err.to_string().contains("desired_tari_output"), "{err}");
        }

        let pair = (
            ResourceAddress::from_array([1; 32]),
            ResourceAddress::from_array([2; 32]),
        );
        for resource_pair in [None, Some(pair)] {
            let resp = handle_list(
                &daemon.context,
                Some(&daemon.session),
                SwapPoolsListRequest {
                    limit: Some(0),
                    offset: Some(0),
                    resource_pair,
                },
            )
            .await
            .unwrap();
            assert!(resp.pools.is_empty());
        }
    }
}
