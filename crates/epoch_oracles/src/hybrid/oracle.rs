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

pub struct HybridEpochOracle<TStore, TBaseLayer = BaseLayerOracle<TStore>> {
    configured: ConfiguredEpochOracle<TStore, MpscEpochTicker>,
    base_layer: TBaseLayer,
    trigger: mpsc::UnboundedSender<EpochTickerData>,
    has_initial_sync_completed: bool,
    last_triggered_epoch: Option<Epoch>,
    /// Number of triggered epoch ticks that the configured oracle has not yet emitted as `EpochChanged`.
    unemitted_triggers: usize,
    /// A `DoneForNow` waiting for all currently triggered epochs to be emitted by the configured oracle.
    deferred_done_for_now: Option<(Epoch, FixedHash)>,
    ready_event: Option<EpochEvent>,
}

impl<TStore: EpochOracleStore + Send + 'static, TBaseLayer> HybridEpochOracle<TStore, TBaseLayer> {
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
            unemitted_triggers: 0,
            deferred_done_for_now: None,
            ready_event: None,
        }
    }
}

impl<TStore: EpochOracleStore + Send + 'static, TBaseLayer: EpochEventOracle + Send + 'static> EpochEventOracle
    for HybridEpochOracle<TStore, TBaseLayer>
{
    async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
        if let Some(event) = self.ready_event.take() {
            return Some(event);
        }
        loop {
            tokio::select! {
                biased;

                event = self.configured.next_epoch_event() => {
                    let event = event?;
                    debug!(target: LOG_TARGET, "📝 Configured epoch event: {}", event);
                    if matches!(&event, EpochEvent::EpochChanged { .. }) {
                        self.unemitted_triggers = self.unemitted_triggers.saturating_sub(1);
                        if self.unemitted_triggers == 0 &&
                            let Some((done_epoch, epoch_hash)) = self.deferred_done_for_now.take()
                        {
                            self.ready_event = Some(EpochEvent::DoneForNow { epoch: done_epoch, epoch_hash });
                        }
                    }
                    return Some(event);
                },
                event = self.base_layer.next_epoch_event() => {
                    let event = event?;
                    debug!(target: LOG_TARGET, "⛓️ Base layer epoch event: {}", event);
                    match event {
                        EpochEvent::Error(err) => return Some(EpochEvent::Error(err)),
                        // Each triggered tick produces one `EpochChanged` from the configured oracle; `DoneForNow`
                        // follows only after the latest queued tick has been emitted.
                        EpochEvent::EpochChanged { epoch, epoch_hash } => {
                            self.last_triggered_epoch = Some(epoch);
                            self.unemitted_triggers += 1;
                            self.deferred_done_for_now = self.has_initial_sync_completed.then_some((epoch, epoch_hash));
                            let _ignore = self.trigger.send(EpochTickerData {
                                epoch,
                                epoch_hash,
                                done_for_now: false,
                            });
                        },
                        EpochEvent::DoneForNow { epoch, epoch_hash } if !self.has_initial_sync_completed => {
                            self.has_initial_sync_completed = true;
                            if self.last_triggered_epoch != Some(epoch) {
                                self.last_triggered_epoch = Some(epoch);
                                self.unemitted_triggers += 1;
                                let _ignore = self.trigger.send(EpochTickerData {
                                    epoch,
                                    epoch_hash,
                                    done_for_now: false,
                                });
                            }
                            // The configured oracle may hold epochs back; scanning is only complete once the epoch
                            // manager has received every triggered epoch.
                            if self.unemitted_triggers == 0 {
                                return Some(EpochEvent::DoneForNow { epoch, epoch_hash });
                            }
                            self.deferred_done_for_now = Some((epoch, epoch_hash));
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
    use std::{collections::HashMap, future::poll_fn, num::NonZeroU64, sync::Mutex, task::Poll};

    use serde::{Serialize, de::DeserializeOwned};
    use tari_epoch_manager::epoch_event_oracle::{EpochEvent, EpochEventOracle, ValidatorNodeChange};
    use tari_ootle_common_types::{Epoch, ShardGroup};
    use tari_sidechain::FixedHash;
    use tari_template_lib::types::crypto::RistrettoPublicKeyBytes;
    use tokio::sync::mpsc;

    use super::HybridEpochOracle;
    use crate::{
        configured::{Config, ConfiguredEpochOracle, Validator},
        hybrid::mpsc_ticker,
        store::EpochOracleStore,
    };

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

    struct MockBaseLayer {
        rx: mpsc::UnboundedReceiver<EpochEvent>,
    }

    impl EpochEventOracle for MockBaseLayer {
        async fn next_epoch_event(&mut self) -> Option<EpochEvent> {
            self.rx.recv().await
        }
    }

    fn mk_hash(byte: u8) -> FixedHash {
        FixedHash::from([byte; 32])
    }

    fn mk_key(seed: u8) -> RistrettoPublicKeyBytes {
        RistrettoPublicKeyBytes::from_bytes(&[seed; 32]).unwrap()
    }

    fn mk_validator(seed: u8, registration_epoch: Epoch, exit_epoch: Option<Epoch>) -> Validator {
        Validator {
            public_key: mk_key(seed),
            claim_key: mk_key(seed.wrapping_add(1)),
            shard_group: ShardGroup::new(1, 256),
            registration_epoch,
            claim_key_changes: vec![],
            power: NonZeroU64::MIN,
            exit_epoch,
        }
    }

    async fn poll_available<TStore, TBaseLayer>(oracle: &mut HybridEpochOracle<TStore, TBaseLayer>) -> Vec<EpochEvent>
    where
        TStore: EpochOracleStore + Send + 'static,
        TBaseLayer: EpochEventOracle + Send + 'static,
    {
        let mut events = Vec::new();
        loop {
            let next = poll_fn(|cx| {
                let fut = oracle.next_epoch_event();
                tokio::pin!(fut);
                Poll::Ready(fut.poll(cx))
            })
            .await;
            match next {
                Poll::Ready(Some(event)) => events.push(event),
                Poll::Ready(None) | Poll::Pending => break,
            }
        }
        events
    }

    #[tokio::test]
    async fn later_epoch_supersedes_deferred_done_for_now_while_configured_oracle_holds_epochs() {
        let initial_config = Config {
            validators: vec![mk_validator(1, Epoch(0), None)],
            ..Config::default()
        };
        let mut updated_config = initial_config.clone();
        updated_config.validators[0].exit_epoch = Some(Epoch(6));

        let (ticker, trigger) = mpsc_ticker();
        let (config_tx, config_rx) = mpsc::unbounded_channel();
        let configured = ConfiguredEpochOracle::with_custom_ticker(initial_config, TestStore::default(), ticker)
            .with_config_updates(config_rx);
        let (base_tx, base_rx) = mpsc::unbounded_channel();
        let mut hybrid = HybridEpochOracle::new(configured, MockBaseLayer { rx: base_rx }, trigger);

        // Initial base-layer scan reaches epoch 5 while the configured oracle waits for the first config update;
        // only the pending registration announcement from startup initialization is emitted before the update arrives.
        base_tx
            .send(EpochEvent::EpochChanged {
                epoch: Epoch(5),
                epoch_hash: mk_hash(5),
            })
            .unwrap();
        base_tx
            .send(EpochEvent::DoneForNow {
                epoch: Epoch(5),
                epoch_hash: mk_hash(5),
            })
            .unwrap();
        let initial_events = poll_available(&mut hybrid).await;
        assert!(
            matches!(initial_events.as_slice(), [EpochEvent::NewValidatorRegistered {
                activation_epoch: Epoch(1),
                ..
            }]),
            "expected only startup registration announcement while epochs are held back; got {initial_events:?}"
        );

        // Before the startup config fetch finishes, the base layer crosses into epoch 6.
        base_tx
            .send(EpochEvent::EpochChanged {
                epoch: Epoch(6),
                epoch_hash: mk_hash(6),
            })
            .unwrap();
        base_tx
            .send(EpochEvent::DoneForNow {
                epoch: Epoch(6),
                epoch_hash: mk_hash(6),
            })
            .unwrap();
        assert!(poll_available(&mut hybrid).await.is_empty());

        // Once the validator config arrives, epoch 5 must not emit a premature DoneForNow before epoch 6's exit and
        // EpochChanged are applied.
        config_tx.send(Some(updated_config)).unwrap();
        let events = poll_available(&mut hybrid).await;

        assert!(
            matches!(
                events.as_slice(),
                [
                    EpochEvent::ActiveValidatorNodeSetChanged { epoch: Epoch(4), .. },
                    EpochEvent::EpochChanged { epoch: Epoch(5), .. },
                    EpochEvent::ActiveValidatorNodeSetChanged {
                        epoch: Epoch(6),
                        node_changes,
                    },
                    EpochEvent::EpochChanged { epoch: Epoch(6), .. },
                    EpochEvent::DoneForNow { epoch: Epoch(6), epoch_hash },
                ] if matches!(node_changes.as_slice(), [ValidatorNodeChange::Remove { public_key }] if *public_key == mk_key(1))
                    && *epoch_hash == mk_hash(6)
            ),
            "unexpected event sequence: {events:?}"
        );
    }

    #[tokio::test]
    async fn reorged_epoch_hash_supersedes_deferred_done_for_now_while_held_back() {
        let (ticker, trigger) = mpsc_ticker();
        let (config_tx, config_rx) = mpsc::unbounded_channel();
        let configured = ConfiguredEpochOracle::with_custom_ticker(Config::default(), TestStore::default(), ticker)
            .with_config_updates(config_rx);
        let (base_tx, base_rx) = mpsc::unbounded_channel();
        let mut hybrid = HybridEpochOracle::new(configured, MockBaseLayer { rx: base_rx }, trigger);

        base_tx
            .send(EpochEvent::EpochChanged {
                epoch: Epoch(3),
                epoch_hash: mk_hash(0xaa),
            })
            .unwrap();
        base_tx
            .send(EpochEvent::DoneForNow {
                epoch: Epoch(3),
                epoch_hash: mk_hash(0xaa),
            })
            .unwrap();
        assert!(poll_available(&mut hybrid).await.is_empty());

        // Shallow L1 reorg replaces epoch 3's boundary hash while epochs are still held back.
        base_tx
            .send(EpochEvent::EpochChanged {
                epoch: Epoch(3),
                epoch_hash: mk_hash(0xbb),
            })
            .unwrap();
        assert!(poll_available(&mut hybrid).await.is_empty());

        config_tx.send(None).unwrap();
        let events = poll_available(&mut hybrid).await;

        assert_eq!(events.len(), 3, "got {events:?}");
        assert!(matches!(
            events[0],
            EpochEvent::EpochChanged { epoch: Epoch(3), epoch_hash } if epoch_hash == mk_hash(0xaa)
        ));
        assert!(matches!(
            events[1],
            EpochEvent::EpochChanged { epoch: Epoch(3), epoch_hash } if epoch_hash == mk_hash(0xbb)
        ));
        assert!(matches!(
            events[2],
            EpochEvent::DoneForNow { epoch: Epoch(3), epoch_hash } if epoch_hash == mk_hash(0xbb)
        ));
    }

    #[tokio::test]
    async fn no_progress_done_for_now_is_superseded_by_later_epoch_while_held_back() {
        let (ticker, trigger) = mpsc_ticker();
        let (config_tx, config_rx) = mpsc::unbounded_channel();
        let configured = ConfiguredEpochOracle::with_custom_ticker(Config::default(), TestStore::default(), ticker)
            .with_config_updates(config_rx);
        let (base_tx, base_rx) = mpsc::unbounded_channel();
        let mut hybrid = HybridEpochOracle::new(configured, MockBaseLayer { rx: base_rx }, trigger);

        // Restart with no new base-layer blocks emits DoneForNow without a preceding EpochChanged.
        base_tx
            .send(EpochEvent::DoneForNow {
                epoch: Epoch(4),
                epoch_hash: mk_hash(4),
            })
            .unwrap();
        assert!(poll_available(&mut hybrid).await.is_empty());

        // Base layer advances to epoch 5 before the startup config fetch completes.
        base_tx
            .send(EpochEvent::EpochChanged {
                epoch: Epoch(5),
                epoch_hash: mk_hash(5),
            })
            .unwrap();
        assert!(poll_available(&mut hybrid).await.is_empty());

        config_tx.send(None).unwrap();
        let events = poll_available(&mut hybrid).await;

        assert_eq!(events.len(), 3, "got {events:?}");
        assert!(matches!(events[0], EpochEvent::EpochChanged { epoch: Epoch(4), .. }));
        assert!(matches!(events[1], EpochEvent::EpochChanged { epoch: Epoch(5), .. }));
        assert!(matches!(
            events[2],
            EpochEvent::DoneForNow { epoch: Epoch(5), epoch_hash } if epoch_hash == mk_hash(5)
        ));
    }
}
