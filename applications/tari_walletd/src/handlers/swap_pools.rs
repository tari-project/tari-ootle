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
    // Page through watched pools from the beginning, skip `offset` matching pools, and collect up to `limit`.
    let fetch_limit = if req.resource_pair.is_some() {
        50u64
    } else {
        limit as u64
    };
    let mut watched_offset = if req.resource_pair.is_some() { 0 } else { offset };
    let mut skipped_matches = 0u64;
    let mut pools = Vec::with_capacity(limit);

    loop {
        let watched = sdk
            .get_network_interface()
            .list_watched_substates(
                Some(LIQUIDITY_POOL_TEMPLATE_ADDRESS),
                Some(fetch_limit),
                Some(watched_offset),
            )
            .await?;
        let batch_len = watched.len();
        if batch_len == 0 {
            break;
        }

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
                        if skipped_matches < offset {
                            skipped_matches += 1;
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

        if req.resource_pair.is_none() || pools.len() >= limit || (batch_len as u64) < fetch_limit {
            break;
        }
        watched_offset = match watched_offset.checked_add(batch_len as u64) {
            Some(next) => next,
            None => break,
        };
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
    use std::{collections::HashMap, sync::Arc};

    use axum::{
        Json,
        Router,
        extract::{Path, Query, State},
        http::StatusCode,
        routing::{get, post},
    };
    use axum_jrpc::error::{JsonRpcError, JsonRpcErrorReason};
    use tari_engine_types::{
        component::{Component, ComponentBody, ComponentHeader},
        encoded::Encoded,
        resource_container::ResourceContainer,
        substate::{EncodedSubstate, Substate, SubstateValue},
        vault::Vault,
    };
    use tari_indexer_client::types::{
        GetSubstateResponse,
        GetSubstatesRequest,
        GetSubstatesResponse,
        ListWatchedSubstatesRequest,
        ListWatchedSubstatesResponse,
        SubstateState,
        WatchedSubstateItem,
    };
    use tari_ootle_common_types::SubstateVersion;
    use tari_ootle_wallet_sdk::IndexerTrust;
    use tari_template_lib_types::{
        EntityId,
        ObjectKey,
        ResourceAddress,
        SubstateOwnerRule,
        VaultId,
        access_rules::{ComponentAccessRules, RulesUpdater, UpdateRule},
    };
    use url::Url;

    use super::*;
    use crate::handlers::test_support::{FakeIndexer, TestDaemon};

    #[tokio::test]
    async fn exchange_rate_rejects_zero_desired_output_and_list_short_circuits_zero_limit() {
        let daemon = TestDaemon::start().await;
        let pool_address = ComponentAddress::from_array([0x44; 32]);

        let err = handle_get_exchange_rate(&daemon.context, Some(&daemon.session), SwapPoolGetExchangeRateRequest {
            pool_address,
            desired_tari_output: Some(Amount::zero()),
        })
        .await
        .unwrap_err();
        let jrpc = err.downcast_ref::<JsonRpcError>().expect("JsonRpcError");
        assert!(matches!(jrpc.error_reason(), JsonRpcErrorReason::InvalidParams));
        assert!(err.to_string().contains("desired_tari_output"), "{err}");

        let pair = (
            ResourceAddress::new(ObjectKey::from_array([1; ObjectKey::LENGTH])),
            ResourceAddress::new(ObjectKey::from_array([2; ObjectKey::LENGTH])),
        );
        for resource_pair in [None, Some(pair)] {
            let resp = handle_list(&daemon.context, Some(&daemon.session), SwapPoolsListRequest {
                limit: Some(0),
                offset: Some(0),
                resource_pair,
            })
            .await
            .unwrap();
            assert!(resp.pools.is_empty());
        }
    }

    #[derive(Clone)]
    struct MockPoolIndexer {
        watched: Vec<WatchedSubstateItem>,
        components: HashMap<SubstateId, Encoded<SubstateValue>>,
        vaults: HashMap<SubstateId, EncodedSubstate>,
    }

    impl MockPoolIndexer {
        fn new() -> Self {
            Self {
                watched: Vec::new(),
                components: HashMap::new(),
                vaults: HashMap::new(),
            }
        }

        fn add_pool(&mut self, seed: u8, resource_a: ResourceAddress, resource_b: ResourceAddress) -> ComponentAddress {
            let pool_addr = ComponentAddress::from_array([seed; 32]);
            let vault_a_id = VaultId::new(ObjectKey::from_array([seed.wrapping_add(100); ObjectKey::LENGTH]));
            let vault_b_id = VaultId::new(ObjectKey::from_array([seed.wrapping_add(200); ObjectKey::LENGTH]));

            let state = tari_bor::to_value(&(vault_a_id, vault_b_id)).unwrap();
            let component = Component {
                header: ComponentHeader {
                    template_address: LIQUIDITY_POOL_TEMPLATE_ADDRESS,
                    owner_rule: SubstateOwnerRule::None,
                    access_rules: ComponentAccessRules::allow_all(),
                    entity_id: EntityId::default(),
                    upgrade_rule: UpdateRule::Locked,
                    rules_updater: Some(RulesUpdater::Locked),
                },
                body: ComponentBody { state },
            };

            let pool_id = SubstateId::Component(pool_addr);
            self.watched.push(WatchedSubstateItem {
                component_address: pool_id.clone(),
                template_address: LIQUIDITY_POOL_TEMPLATE_ADDRESS,
            });
            self.components
                .insert(pool_id, Encoded::encode(&SubstateValue::Component(component)).unwrap());

            let vault_a = Vault::new(ResourceContainer::public_fungible(resource_a, Amount::from(100u64)));
            let vault_b = Vault::new(ResourceContainer::public_fungible(resource_b, Amount::from(200u64)));
            self.vaults.insert(
                SubstateId::Vault(vault_a_id),
                EncodedSubstate::encode(&Substate::new(0, SubstateValue::Vault(vault_a))).unwrap(),
            );
            self.vaults.insert(
                SubstateId::Vault(vault_b_id),
                EncodedSubstate::encode(&Substate::new(0, SubstateValue::Vault(vault_b))).unwrap(),
            );

            pool_addr
        }

        async fn spawn(self) -> Url {
            let state = Arc::new(self);
            let router = Router::new()
                .route("/substates/watched", get(Self::list_watched))
                .route("/substates/fetch", post(Self::fetch_substates))
                .route("/substates/{id}", get(Self::get_substate))
                .with_state(state);
            let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
            let url = Url::parse(&format!("http://{}/", listener.local_addr().unwrap())).unwrap();
            tokio::spawn(async move { axum::serve(listener, router).await });
            url
        }

        async fn list_watched(
            State(state): State<Arc<Self>>,
            Query(req): Query<ListWatchedSubstatesRequest>,
        ) -> Json<ListWatchedSubstatesResponse> {
            let offset = req.offset.unwrap_or(0) as usize;
            let limit = req.limit.unwrap_or(50) as usize;
            let substates = state.watched.iter().skip(offset).take(limit).cloned().collect();
            Json(ListWatchedSubstatesResponse { substates })
        }

        async fn get_substate(
            State(state): State<Arc<Self>>,
            Path(id): Path<SubstateId>,
        ) -> Result<Json<GetSubstateResponse>, StatusCode> {
            let substate = state.components.get(&id).cloned().ok_or(StatusCode::NOT_FOUND)?;
            Ok(Json(GetSubstateResponse {
                version: SubstateVersion::ZERO,
                state: SubstateState::Up { substate },
                verified: true,
                proof: None,
            }))
        }

        async fn fetch_substates(
            State(state): State<Arc<Self>>,
            Json(req): Json<GetSubstatesRequest>,
        ) -> Json<GetSubstatesResponse> {
            let mut substates = HashMap::new();
            for id in req.requests.as_slice() {
                if let Some(substate) = state.vaults.get(id) {
                    substates.insert(id.clone(), substate.clone());
                }
            }
            Json(GetSubstatesResponse::new(substates, []))
        }
    }

    #[tokio::test]
    async fn handle_list_applies_offset_to_filtered_resource_pair_matches() {
        let r1 = ResourceAddress::new(ObjectKey::from_array([1; ObjectKey::LENGTH]));
        let r2 = ResourceAddress::new(ObjectKey::from_array([2; ObjectKey::LENGTH]));
        let r3 = ResourceAddress::new(ObjectKey::from_array([3; ObjectKey::LENGTH]));

        let mut mock = MockPoolIndexer::new();
        // Watched pool 0 does not match (r1, r2); pools 1 and 2 match.
        let _unmatched = mock.add_pool(10, r1, r3);
        let match_first = mock.add_pool(11, r1, r2);
        let match_second = mock.add_pool(12, r2, r1);
        let url = mock.spawn().await;

        let fake = FakeIndexer::spawn().await;
        let daemon = TestDaemon::start_with_indexer(&fake, IndexerTrust::Trust).await;
        daemon
            .context
            .wallet_sdk()
            .get_network_interface()
            .set_endpoints(vec![url])
            .unwrap();

        let page_0 = handle_list(&daemon.context, Some(&daemon.session), SwapPoolsListRequest {
            limit: Some(1),
            offset: Some(0),
            resource_pair: Some((r1, r2)),
        })
        .await
        .unwrap();
        assert_eq!(page_0.pools.len(), 1);
        assert_eq!(page_0.pools[0].pool_address, match_first);

        let page_1 = handle_list(&daemon.context, Some(&daemon.session), SwapPoolsListRequest {
            limit: Some(1),
            offset: Some(1),
            resource_pair: Some((r1, r2)),
        })
        .await
        .unwrap();
        assert_eq!(page_1.pools.len(), 1);
        assert_eq!(page_1.pools[0].pool_address, match_second);

        let page_2 = handle_list(&daemon.context, Some(&daemon.session), SwapPoolsListRequest {
            limit: Some(1),
            offset: Some(2),
            resource_pair: Some((r1, r2)),
        })
        .await
        .unwrap();
        assert!(page_2.pools.is_empty());
    }
}
