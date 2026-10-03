//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::future::Future;

use futures::{StreamExt, stream::FuturesUnordered};
use tari_ootle_common_types::SubstateVersion;
use tari_validator_node_rpc::client::{SubstateProofData, SubstateResult};

use crate::{cached_substate_manager::SubstateLookupResult, error::IndexerError};

const LOG_TARGET: &str = "tari::indexer::scanner";

/// How many committee members a single-substate read keeps in flight at once.
///
/// A member that is down costs a full connect timeout before it answers with an error, so a read
/// that asks one member at a time stalls for that long whenever the first pick is dead. Asking a few
/// at once bounds the stall to the slowest of the in-flight members that are actually up, at the
/// cost of that many concurrent requests per read.
pub const READ_RACE_WIDTH: usize = 3;

/// One member's answer, with the proof it verified against the committee. `None` when the member
/// could not prove it, or when proofs are not being verified.
pub type MemberResponse = Result<(SubstateResult, Option<SubstateProofData>), IndexerError>;

/// Folds committee members' responses to a single-substate read into an answer.
///
/// Responses arrive in whatever order the members answer, so the tally cannot assume anything about
/// which member it hears from first. What settles a read is decided per response: a proven `Up`, a
/// proven `Down` of the version the read asked for (or any `Up`/`Down` while proofs are not
/// required) answers on the spot, `DoesNotExist` answers only once more than `f` members agree, and
/// any other `Up`/`Down` is held as a fallback in case no member can prove one.
///
/// A proof of a `Down` shows only that the version it names is not up. That answers a read for that
/// version, but says nothing about whether a later version is up, so it cannot answer a read for the
/// head.
#[derive(Debug)]
pub struct CommitteeReadTally {
    /// Byzantine tolerance of the committee: `DoesNotExist` needs `f + 1` agreeing members before it
    /// is believed, since any `f` of them may be lying or behind.
    f: usize,
    verify_substate_proofs: bool,
    /// The version the read asked for, or `None` for the head.
    requested_version: Option<SubstateVersion>,
    num_nexist: usize,
    last_error: Option<IndexerError>,
    /// Highest-version `Up`/`Down` response that came back without a proof. Only served if no
    /// member can prove one.
    unproven_result: Option<SubstateResult>,
}

impl CommitteeReadTally {
    pub fn new(
        committee_size: usize,
        verify_substate_proofs: bool,
        requested_version: Option<SubstateVersion>,
    ) -> Self {
        Self {
            f: committee_size.saturating_sub(1) / 3,
            verify_substate_proofs,
            requested_version,
            num_nexist: 0,
            last_error: None,
            unproven_result: None,
        }
    }

    /// Folds in one member's response, returning the answer if this response settles the read.
    pub fn observe(&mut self, response: MemberResponse) -> Option<SubstateLookupResult> {
        match response {
            Ok((substate_result, proof)) => match substate_result {
                SubstateResult::Up { .. } | SubstateResult::Down { .. } => {
                    if !self.verify_substate_proofs || (proof.is_some() && self.proof_answers_read(&substate_result)) {
                        return Some(SubstateLookupResult {
                            result: substate_result,
                            verified: proof.is_some(),
                            proof,
                        });
                    }
                    // The member could not prove an answer to this read (e.g. nothing committed
                    // since the epoch started, or a down version other than the one asked for). Keep
                    // the highest version as an unverified fallback (a member that is still syncing
                    // may respond with a stale copy) and wait on the rest of the committee for a
                    // proven copy.
                    if self
                        .unproven_result
                        .as_ref()
                        .is_none_or(|r| r.version() < substate_result.version())
                    {
                        self.unproven_result = Some(substate_result);
                    }
                    None
                },
                SubstateResult::DoesNotExist => {
                    self.num_nexist += 1;
                    (self.num_nexist > self.f).then_some(SubstateLookupResult {
                        result: SubstateResult::DoesNotExist,
                        verified: false,
                        proof: None,
                    })
                },
            },
            Err(e) => {
                // A single member's error is ignored while the rest of the committee may still answer.
                self.last_error = Some(e);
                None
            },
        }
    }

    /// Whether a proof of `result` answers this read. See [`CommitteeReadTally`].
    fn proof_answers_read(&self, result: &SubstateResult) -> bool {
        match result {
            SubstateResult::Down { version } => self.requested_version == Some(*version),
            SubstateResult::Up { .. } | SubstateResult::DoesNotExist => true,
        }
    }

    /// The answer once every member has responded without settling the read.
    pub fn conclude(self, describe: impl std::fmt::Display) -> Result<SubstateLookupResult, IndexerError> {
        if let Some(result) = self.unproven_result {
            log::warn!(
                target: LOG_TARGET,
                "No committee member could supply a proof for {describe}. Returning the substate unverified.",
            );
            return Ok(SubstateLookupResult {
                result,
                verified: false,
                proof: None,
            });
        }

        log::warn!(
            target: LOG_TARGET,
            "Could not get substate {describe} from any of the validator nodes",
        );

        if let Some(e) = self.last_error {
            return Err(e);
        }
        Ok(SubstateLookupResult {
            result: SubstateResult::DoesNotExist,
            verified: false,
            proof: None,
        })
    }
}

/// Drives one request per committee member, at most `width` at a time, until a response settles
/// the read.
///
/// `requests` is drawn from lazily: a member is asked as soon as a slot frees up, so an unresponsive
/// member holds one slot for as long as its request takes and delays nothing else. Requests still
/// in flight when the read settles are dropped.
pub async fn race_committee<I, D>(
    requests: I,
    width: usize,
    mut tally: CommitteeReadTally,
    describe: D,
) -> Result<SubstateLookupResult, IndexerError>
where
    I: IntoIterator,
    I::Item: Future<Output = MemberResponse>,
    D: std::fmt::Display,
{
    let mut requests = requests.into_iter();
    let mut in_flight = FuturesUnordered::new();
    in_flight.extend(requests.by_ref().take(width.max(1)));

    while let Some(response) = in_flight.next().await {
        if let Some(answer) = tally.observe(response) {
            return Ok(answer);
        }
        if let Some(request) = requests.next() {
            in_flight.push(request);
        }
    }

    tally.conclude(describe)
}

#[cfg(test)]
mod tests {
    use std::{
        collections::HashMap,
        sync::{
            Arc,
            atomic::{AtomicUsize, Ordering},
        },
        time::Duration,
    };

    use futures::future::{BoxFuture, FutureExt};
    use tari_engine_types::{
        non_fungible::NonFungibleContainer,
        substate::{Substate, SubstateValue},
    };

    use super::*;

    fn down(version: u64) -> SubstateResult {
        SubstateResult::Down {
            version: SubstateVersion::new(version),
        }
    }

    fn up(version: u64) -> SubstateResult {
        SubstateResult::Up {
            substate: Box::new(Substate::new(
                version,
                SubstateValue::NonFungible(NonFungibleContainer::no_data()),
            )),
        }
    }

    fn proven() -> Option<SubstateProofData> {
        Some(SubstateProofData {
            substate_value_proof: vec![],
            commit_proof: vec![],
            proof_epoch: 0,
        })
    }

    fn error() -> IndexerError {
        IndexerError::ValidatorNodeClientError("unreachable".into())
    }

    /// A committee whose member `n` answers with `responses[n]`, or never answers when there is no
    /// entry for it.
    struct Committee {
        responses: HashMap<usize, MemberResponse>,
        started: Arc<AtomicUsize>,
    }

    impl Committee {
        fn new(responses: Vec<(usize, MemberResponse)>) -> Self {
            Self {
                responses: responses.into_iter().collect(),
                started: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn fetch(&mut self) -> impl Fn(usize) -> BoxFuture<'static, MemberResponse> + use<> {
            let started = self.started.clone();
            // Each member answers at most once, so its response is handed over rather than cloned.
            let responses = Arc::new(std::sync::Mutex::new(std::mem::take(&mut self.responses)));
            move |member| {
                started.fetch_add(1, Ordering::SeqCst);
                match responses.lock().unwrap().remove(&member) {
                    Some(response) => futures::future::ready(response).boxed(),
                    None => futures::future::pending().boxed(),
                }
            }
        }
    }

    async fn race(
        size: usize,
        width: usize,
        verify: bool,
        requested_version: Option<u64>,
        responses: Vec<(usize, MemberResponse)>,
    ) -> Result<SubstateLookupResult, IndexerError> {
        let mut committee = Committee::new(responses);
        let fetch = committee.fetch();
        race_committee(
            (0..size).map(fetch),
            width,
            CommitteeReadTally::new(size, verify, requested_version.map(SubstateVersion::new)),
            "test",
        )
        .await
    }

    #[tokio::test]
    async fn a_member_that_never_answers_does_not_delay_the_rest() {
        let result = tokio::time::timeout(
            Duration::from_secs(1),
            race(3, READ_RACE_WIDTH, true, Some(4), vec![(1, Ok((down(4), proven())))]),
        )
        .await
        .expect("read stalled behind an unresponsive member")
        .unwrap();
        assert_eq!(result.result.version(), Some(SubstateVersion::new(4)));
        assert!(result.verified);
    }

    #[tokio::test]
    async fn no_more_than_the_window_is_in_flight() {
        let mut committee = Committee::new(vec![]);
        let started = committee.started.clone();
        let fetch = committee.fetch();
        let read = race_committee((0..5).map(fetch), 2, CommitteeReadTally::new(5, true, None), "test");
        assert!(
            tokio::time::timeout(Duration::from_millis(50), read).await.is_err(),
            "nothing answered, so the read cannot have settled"
        );
        assert_eq!(started.load(Ordering::SeqCst), 2);
    }

    #[tokio::test]
    async fn a_failed_member_frees_its_slot_for_the_next() {
        let result = race(3, 1, true, Some(2), vec![
            (0, Err(error())),
            (1, Err(error())),
            (2, Ok((down(2), proven()))),
        ])
        .await
        .unwrap();
        assert_eq!(result.result.version(), Some(SubstateVersion::new(2)));
    }

    #[tokio::test]
    async fn an_unproven_answer_is_held_until_a_proof_arrives() {
        // Member 0 answers first and cannot prove; member 1 can.
        let result = race(2, 1, true, Some(7), vec![
            (0, Ok((down(7), None))),
            (1, Ok((down(7), proven()))),
        ])
        .await
        .unwrap();
        assert!(result.verified);
    }

    /// A down version says nothing about whether a later one is up, so proving one does not settle a
    /// read for the head while another member can prove what the head is.
    #[tokio::test]
    async fn a_proven_down_does_not_answer_a_read_for_the_head() {
        let result = race(2, 1, true, None, vec![
            (0, Ok((down(3), proven()))),
            (1, Ok((up(4), proven()))),
        ])
        .await
        .unwrap();
        assert!(matches!(result.result, SubstateResult::Up { .. }));
        assert_eq!(result.result.version(), Some(SubstateVersion::new(4)));
        assert!(result.verified);
    }

    #[tokio::test]
    async fn a_proven_down_alone_leaves_a_read_for_the_head_unverified() {
        let result = race(1, 1, true, None, vec![(0, Ok((down(3), proven())))])
            .await
            .unwrap();
        assert_eq!(result.result.version(), Some(SubstateVersion::new(3)));
        assert!(!result.verified);
        assert!(result.proof.is_none());
    }

    #[tokio::test]
    async fn a_proven_down_of_another_version_does_not_answer_a_versioned_read() {
        let result = race(2, 1, true, Some(5), vec![
            (0, Ok((down(6), proven()))),
            (1, Ok((down(5), proven()))),
        ])
        .await
        .unwrap();
        assert_eq!(result.result.version(), Some(SubstateVersion::new(5)));
        assert!(result.verified);
    }

    #[tokio::test]
    async fn a_proven_answer_carries_its_proof() {
        let result = race(2, 1, true, Some(3), vec![(0, Ok((down(3), proven())))])
            .await
            .unwrap();
        assert!(result.verified);
        assert!(result.proof.is_some());
    }

    #[tokio::test]
    async fn the_highest_unproven_version_is_served_when_nobody_can_prove() {
        let result = race(3, 1, true, None, vec![
            (0, Ok((down(2), None))),
            (1, Ok((down(5), None))),
            (2, Ok((down(3), None))),
        ])
        .await
        .unwrap();
        assert_eq!(result.result.version(), Some(SubstateVersion::new(5)));
        assert!(!result.verified);
    }

    #[tokio::test]
    async fn an_unproven_answer_settles_the_read_when_proofs_are_not_required() {
        let result = race(2, 1, false, None, vec![(0, Ok((down(1), None)))]).await.unwrap();
        assert_eq!(result.result.version(), Some(SubstateVersion::new(1)));
        assert!(!result.verified);
    }

    #[tokio::test]
    async fn nonexistence_needs_more_than_f_agreeing_members() {
        // Four members: f = 1, so two must agree.
        let result = race(4, 1, true, None, vec![
            (0, Ok((SubstateResult::DoesNotExist, None))),
            (1, Ok((SubstateResult::DoesNotExist, None))),
        ])
        .await
        .unwrap();
        assert!(matches!(result.result, SubstateResult::DoesNotExist));
    }

    #[tokio::test]
    async fn a_single_nonexistence_is_outvoted_by_a_proven_version() {
        let result = race(4, 1, true, Some(1), vec![
            (0, Ok((SubstateResult::DoesNotExist, None))),
            (1, Ok((down(1), proven()))),
        ])
        .await
        .unwrap();
        assert_eq!(result.result.version(), Some(SubstateVersion::new(1)));
    }

    #[tokio::test]
    async fn f_agreeing_members_and_errors_from_the_rest_is_the_last_error() {
        let result = race(4, 1, true, None, vec![
            (0, Ok((SubstateResult::DoesNotExist, None))),
            (1, Err(error())),
            (2, Err(error())),
            (3, Err(error())),
        ])
        .await;
        assert!(matches!(result, Err(IndexerError::ValidatorNodeClientError(_))));
    }

    /// Agreement takes precedence over errors from members that could not be reached.
    #[tokio::test]
    async fn agreed_nonexistence_outranks_errors() {
        let result = race(4, 1, true, None, vec![
            (0, Err(error())),
            (1, Ok((SubstateResult::DoesNotExist, None))),
            (2, Err(error())),
            (3, Ok((SubstateResult::DoesNotExist, None))),
        ])
        .await
        .unwrap();
        assert!(matches!(result.result, SubstateResult::DoesNotExist));
    }
}
