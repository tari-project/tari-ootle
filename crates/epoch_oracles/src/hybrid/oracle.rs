//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use log::*;
use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle};
use tari_ootle_common_types::Epoch;
use tari_sidechain::FixedHash;
use tokio::sync::mpsc;

use crate::{
    base_layer::BaseLayerOracle,
    configured::{ConfiguredEpochOracle, EpochTickerData},
    hybrid::mpsc_ticker::MpscEpochTicker,
    store::EpochOracleStore,
};

const LOG_TARGET: &str = "tari::ootle::epoch_oracles::hybrid";

pub struct HybridEpochOracle<TStore, TOracle = BaseLayerOracle<TStore>> {
    configured: ConfiguredEpochOracle<TStore, MpscEpochTicker>,
    base_layer: TOracle,
    trigger: mpsc::UnboundedSender<EpochTickerData>,
    last_triggered_epoch: Option<Epoch>,
}

impl<TStore, TOracle> HybridEpochOracle<TStore, TOracle> {
    pub fn new(
        configured: ConfiguredEpochOracle<TStore, MpscEpochTicker>,
        base_layer: TOracle,
        trigger: mpsc::UnboundedSender<EpochTickerData>,
    ) -> Self {
        Self {
            configured,
            base_layer,
            trigger,
            last_triggered_epoch: None,
        }
    }
}

impl<TStore, TOracle> EpochEventOracle for HybridEpochOracle<TStore, TOracle>
where
    TStore: EpochOracleStore + Send + 'static,
    TOracle: EpochEventOracle + Send,
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
                        EpochEvent::EpochChanged { epoch, epoch_hash } => {
                            self.last_triggered_epoch = Some(epoch);
                            let _ignore = self.trigger.send(EpochTickerData {
                                epoch,
                                epoch_hash,
                                done_for_now: false,
                            });
                        },
                        EpochEvent::DoneForNow { epoch, epoch_hash } => {
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
                        EpochEvent::Error(err) => {
                            return Some(EpochEvent::Error(err));
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
    };

    use serde::{Serialize, de::DeserializeOwned};

    use super::*;
    use crate::{configured::Config, hybrid::mpsc_ticker};

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

    struct ScriptedOracle {
        events: VecDeque<EpochEvent>,
    }

    impl EpochEventOracle for ScriptedOracle {
        async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
            self.events.pop_front()
        }
    }

    #[tokio::test]
    async fn catch_up_does_not_duplicate_final_epoch_change_and_forwards_done_for_now() {
        let (ticker, trigger) = mpsc_ticker();
        let configured = ConfiguredEpochOracle::with_custom_ticker(Config::default(), TestStore::default(), ticker);
        let h1 = FixedHash::from([1u8; 32]);
        let h2 = FixedHash::from([2u8; 32]);
        let base_layer = ScriptedOracle {
            events: VecDeque::from([
                EpochEvent::EpochChanged {
                    epoch: Epoch(1),
                    epoch_hash: h1,
                },
                EpochEvent::EpochChanged {
                    epoch: Epoch(2),
                    epoch_hash: h2,
                },
                EpochEvent::DoneForNow {
                    epoch: Epoch(2),
                    epoch_hash: h2,
                },
            ]),
        };
        let mut hybrid = HybridEpochOracle::new(configured, base_layer, trigger);

        let e1 = hybrid.next_epoch_event().await.unwrap();
        let e2 = hybrid.next_epoch_event().await.unwrap();
        let e3 = hybrid.next_epoch_event().await.unwrap();
        assert!(matches!(e1, EpochEvent::EpochChanged { epoch: Epoch(1), epoch_hash } if epoch_hash == h1));
        assert!(matches!(e2, EpochEvent::EpochChanged { epoch: Epoch(2), epoch_hash } if epoch_hash == h2));
        assert!(matches!(e3, EpochEvent::DoneForNow { epoch: Epoch(2), epoch_hash } if epoch_hash == h2));
        assert!(hybrid.next_epoch_event().await.is_none());
    }

    #[tokio::test]
    async fn base_layer_errors_are_forwarded() {
        let (ticker, trigger) = mpsc_ticker();
        let configured = ConfiguredEpochOracle::with_custom_ticker(Config::default(), TestStore::default(), ticker);
        let base_layer = ScriptedOracle {
            events: VecDeque::from([EpochEvent::error(anyhow::anyhow!("base node unreachable"))]),
        };
        let mut hybrid = HybridEpochOracle::new(configured, base_layer, trigger);

        let event = hybrid.next_epoch_event().await.unwrap();
        assert!(matches!(event, EpochEvent::Error(_)));
    }
}
