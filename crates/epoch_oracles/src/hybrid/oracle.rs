//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};
use tari_ootle_common_types::Epoch;
use tari_sidechain::FixedHash;
use tokio::sync::mpsc;

use crate::{
    base_layer::{BaseLayerBlockHeaderStore, BaseLayerOracle},
    configured::{ConfiguredEpochOracle, EpochTickerData},
    hybrid::mpsc_ticker::MpscEpochTicker,
    store::EpochOracleStore,
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::hybrid";

pub struct HybridEpochOracle<TStore, TBaseLayer = BaseLayerOracle<TStore>> {
    configured: ConfiguredEpochOracle<TStore, MpscEpochTicker>,
    base_layer: TBaseLayer,
    trigger: mpsc::UnboundedSender<EpochTickerData>,
    has_initial_sync_completed: bool,
    last_triggered_epoch: Option<Epoch>,
}

impl<TStore, TBaseLayer> HybridEpochOracle<TStore, TBaseLayer>
where
    TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + 'static,
    TBaseLayer: EpochEventOracle + Send + 'static,
{
    pub fn new(
        configured: ConfiguredEpochOracle<TStore, MpscEpochTicker>,
        base_layer: TBaseLayer,
        trigger: mpsc::UnboundedSender<EpochTickerData>,
    ) -> Self {
        Self {
            configured,
            base_layer,
            trigger,
            has_initial_sync_completed: false,
            last_triggered_epoch: None,
        }
    }
}

impl<TStore, TBaseLayer> EpochEventOracle for HybridEpochOracle<TStore, TBaseLayer>
where
    TStore: EpochOracleStore + BaseLayerBlockHeaderStore + Send + 'static,
    TBaseLayer: EpochEventOracle + Send + 'static,
{
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        loop {
            tokio::select! {
                biased;

                event = self.configured.next_epoch_event() => {
                    let event = event?;
                    debug!(target: LOG_TARGET, "📝 Configured epoch event: {}", event);
                    return Some(event);
                },
                event = self.base_layer.next_epoch_event() => {
                    let event = event?;
                    debug!(target: LOG_TARGET, "⛓️ Base layer epoch event: {}", event);
                    match event {
                        EpochEvent::Error(err) => return Some(EpochEvent::Error(err)),
                        // Trigger the ticker sender for epoch changes
                        EpochEvent::EpochChanged { epoch, epoch_hash } => {
                            self.last_triggered_epoch = Some(epoch);
                            let _ignore = self.trigger.send(EpochTickerData {
                                epoch,
                                epoch_hash,
                                done_for_now: self.has_initial_sync_completed,
                            });
                        },
                        EpochEvent::DoneForNow { epoch, epoch_hash } if !self.has_initial_sync_completed => {
                            self.has_initial_sync_completed = true;
                            if self.last_triggered_epoch == Some(epoch) {
                                return Some(EpochEvent::DoneForNow { epoch, epoch_hash });
                            }
                            self.last_triggered_epoch = Some(epoch);
                            let _ignore = self.trigger.send(EpochTickerData {
                                epoch,
                                epoch_hash,
                                done_for_now: true,
                            });
                        },
                        // Ignore other events that are not epoch changes
                        _ => {},
                    }
                },
            }
        }
    }

    fn observed_epoch_boundary_hash(&self, epoch: Epoch) -> anyhow::Result<Option<FixedHash>> {
        // Epoch boundaries in hybrid mode are scanned by the base-layer oracle; defer to it.
        self.base_layer.observed_epoch_boundary_hash(epoch)
    }
}

#[cfg(test)]
mod tests {
    use std::{
        collections::{HashMap, VecDeque},
        sync::Mutex,
        time::Duration,
    };

    use serde::{Serialize, de::DeserializeOwned};
    use tari_ootle_storage::global::BlockHeaderModel;

    use super::*;
    use crate::configured::Config;

    #[derive(Default)]
    struct TestStore {
        data: Mutex<HashMap<Vec<u8>, Vec<u8>>>,
    }

    impl EpochOracleStore for TestStore {
        fn get<T: DeserializeOwned>(&self, key: &[u8]) -> anyhow::Result<Option<T>> {
            let data = self.data.lock().unwrap();
            data.get(key).map(|v| Ok(serde_json::from_slice(v)?)).transpose()
        }

        fn set<T: Serialize>(&self, key: &[u8], value: &T) -> anyhow::Result<()> {
            let mut data = self.data.lock().unwrap();
            data.insert(key.to_vec(), serde_json::to_vec(value)?);
            Ok(())
        }
    }

    impl BaseLayerBlockHeaderStore for TestStore {
        fn insert_block_headers<I: IntoIterator<Item = BlockHeaderModel>>(&self, _headers: I) -> anyhow::Result<()> {
            Ok(())
        }

        fn get_block_header_by_height(&self, _height: u64) -> anyhow::Result<Option<BlockHeaderModel>> {
            Ok(None)
        }
    }

    struct ScriptedBaseLayerOracle {
        events: VecDeque<EpochEvent>,
    }

    impl EpochEventOracle for ScriptedBaseLayerOracle {
        async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
            self.events.pop_front()
        }
    }

    fn mk_config() -> Config {
        Config {
            epoch_time: Some(Duration::from_secs(1)),
            initial_epoch: Epoch(0),
            base_time: time::OffsetDateTime::now_utc(),
            validators: vec![],
        }
    }

    #[tokio::test]
    async fn done_for_now_after_epoch_changed_does_not_emit_duplicate_epoch_changed() {
        let hash_1 = FixedHash::from([1u8; FixedHash::byte_size()]);
        let hash_2 = FixedHash::from([2u8; FixedHash::byte_size()]);
        let base_layer = ScriptedBaseLayerOracle {
            events: VecDeque::from(vec![
                EpochEvent::EpochChanged {
                    epoch: Epoch(1),
                    epoch_hash: hash_1,
                },
                EpochEvent::DoneForNow {
                    epoch: Epoch(1),
                    epoch_hash: hash_1,
                },
                EpochEvent::EpochChanged {
                    epoch: Epoch(2),
                    epoch_hash: hash_2,
                },
            ]),
        };

        let (ticker, trigger) = MpscEpochTicker::create();
        let configured = ConfiguredEpochOracle::with_custom_ticker(mk_config(), TestStore::default(), ticker);
        let mut hybrid = HybridEpochOracle::new(configured, base_layer, trigger);

        let mut events = Vec::new();
        while let Some(ev) = hybrid.next_epoch_event().await {
            events.push(ev);
        }

        assert!(
            matches!(
                events.as_slice(),
                [
                    EpochEvent::EpochChanged { epoch: Epoch(1), epoch_hash: h1 },
                    EpochEvent::DoneForNow { epoch: Epoch(1), epoch_hash: h1_done },
                    EpochEvent::EpochChanged { epoch: Epoch(2), epoch_hash: h2 },
                    EpochEvent::DoneForNow { epoch: Epoch(2), epoch_hash: h2_done },
                ] if *h1 == hash_1 && *h1_done == hash_1 && *h2 == hash_2 && *h2_done == hash_2
            ),
            "unexpected hybrid oracle event sequence: {events:?}"
        );
    }

    #[tokio::test]
    async fn done_for_now_without_prior_epoch_changed_still_triggers_configured_oracle() {
        let hash_0 = FixedHash::from([9u8; FixedHash::byte_size()]);
        let base_layer = ScriptedBaseLayerOracle {
            events: VecDeque::from(vec![EpochEvent::DoneForNow {
                epoch: Epoch(0),
                epoch_hash: hash_0,
            }]),
        };

        let (ticker, trigger) = MpscEpochTicker::create();
        let configured = ConfiguredEpochOracle::with_custom_ticker(mk_config(), TestStore::default(), ticker);
        let mut hybrid = HybridEpochOracle::new(configured, base_layer, trigger);

        let mut events = Vec::new();
        while let Some(ev) = hybrid.next_epoch_event().await {
            events.push(ev);
        }

        assert!(
            matches!(
                events.as_slice(),
                [
                    EpochEvent::EpochChanged { epoch: Epoch(0), epoch_hash: h0 },
                    EpochEvent::DoneForNow { epoch: Epoch(0), epoch_hash: h0_done },
                ] if *h0 == hash_0 && *h0_done == hash_0
            ),
            "unexpected hybrid oracle event sequence: {events:?}"
        );
    }
}
