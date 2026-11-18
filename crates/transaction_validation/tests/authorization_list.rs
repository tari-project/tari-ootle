//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! A sealer holding one co-signature cannot turn it into a second valid transaction.
//!
//! The victim authorizes a body that withdraws from its account through an unversioned input,
//! naming the sealer. The sealer alone assembles the authorization list, so it controls the list's
//! order, repeats and extra entries. Every such variant must be refused by the structural
//! validator; otherwise each is a distinct transaction id and the withdrawal runs again.

use tari_common_types::types::PrivateKey;
use tari_crypto::{keys::PublicKey as _, ristretto::RistrettoPublicKey, tari_utilities::ByteArray};
use tari_ootle_common_types::{Epoch, InputDeclaration};
use tari_ootle_transaction::{
    AuthorizationContext,
    AuthorizerSet,
    MainSigner,
    Network,
    SchemaVersion,
    Transaction,
    TransactionSignature,
    UnsealedTransactionV1,
    UnsignedTransactionV1,
    args,
    v1_signing,
};
use tari_ootle_transaction_validation::{
    TransactionValidationError,
    Validator,
    create_structural_transaction_validator,
};
use tari_template_lib::types::{Amount, ComponentAddress, crypto::RistrettoPublicKeyBytes};

const MAX_TRANSACTION_WEIGHT: u64 = 100_000;
const MAX_TRANSACTION_SIZE_BYTES: usize = 1024 * 1024;

fn public_key(secret: &PrivateKey) -> RistrettoPublicKeyBytes {
    RistrettoPublicKeyBytes::from_bytes(RistrettoPublicKey::from_secret_key(secret).as_bytes()).unwrap()
}

struct Scenario {
    victim: PrivateKey,
    sealer: PrivateKey,
    body: UnsignedTransactionV1,
}

impl Scenario {
    fn new() -> Self {
        let victim = PrivateKey::from(2u64);
        let sealer = PrivateKey::from(3u64);
        let account = ComponentAddress::from_array([7; 32]);
        let unsigned = Transaction::builder_localnet(Epoch(10))
            .with_main_signer(MainSigner::Authorizer(public_key(&victim)))
            .pay_fee_from_component(account, Amount::new(1000))
            .call_method(account, "withdraw", args![Amount::new(500)])
            .add_input(InputDeclaration::write(account))
            .build_unsigned();
        assert_eq!(unsigned.schema(), SchemaVersion::V2);
        let body = unsigned.into_body();
        assert!(body.has_inputs_without_version());
        Self { victim, sealer, body }
    }

    fn seal_signer(&self) -> RistrettoPublicKeyBytes {
        public_key(&self.sealer)
    }

    /// `secret`'s authorization over the body, signed under the set `keys`.
    fn sign(&self, secret: &PrivateKey, keys: &[RistrettoPublicKeyBytes]) -> TransactionSignature {
        let seal_signer = self.seal_signer();
        let authorizers: AuthorizerSet = keys.iter().collect();
        TransactionSignature::sign(
            secret,
            AuthorizationContext::new(&seal_signer, &authorizers),
            &self.body,
        )
    }

    /// The victim's one co-signature, over the set holding only its own key.
    fn victim_signature(&self) -> TransactionSignature {
        self.sign(&self.victim, &[public_key(&self.victim)])
    }

    /// Seals `signatures` exactly as given, as a sealer assembling its own list would.
    fn seal(&self, signatures: Vec<TransactionSignature>) -> Transaction {
        UnsealedTransactionV1::new(self.body.clone(), signatures).seal(&self.sealer)
    }
}

fn validate(transaction: &Transaction) -> Result<(), TransactionValidationError> {
    create_structural_transaction_validator(Network::LocalNet, MAX_TRANSACTION_WEIGHT, MAX_TRANSACTION_SIZE_BYTES)
        .validate(&(), transaction)
}

#[test]
fn the_honest_transaction_is_accepted() {
    let scenario = Scenario::new();
    let honest = scenario.seal(vec![scenario.victim_signature()]);
    validate(&honest).unwrap();
    assert_eq!(honest.main_signer(), Some(public_key(&scenario.victim)));
}

#[test]
fn a_repeated_co_signature_is_refused() {
    let scenario = Scenario::new();
    let signature = scenario.victim_signature();
    for copies in 2..=5 {
        let err = validate(&scenario.seal(vec![signature.clone(); copies])).unwrap_err();
        assert!(
            matches!(err, TransactionValidationError::AuthorizationsNotCanonical { .. }),
            "{copies} copies: {err}"
        );
    }
}

/// Two authorizers that both signed over their joint set: the list verifies in ascending order and
/// is refused in the other.
#[test]
fn a_permuted_authorization_list_is_refused() {
    let scenario = Scenario::new();
    let other = PrivateKey::from(4u64);
    let keys = [public_key(&scenario.victim), public_key(&other)];
    let mut list = vec![scenario.sign(&scenario.victim, &keys), scenario.sign(&other, &keys)];
    list.sort_by_key(|sig| *sig.public_key());

    validate(&scenario.seal(list.clone())).unwrap();

    list.reverse();
    let err = validate(&scenario.seal(list)).unwrap_err();
    assert!(
        matches!(err, TransactionValidationError::AuthorizationsNotCanonical { .. }),
        "{err}"
    );
}

/// The sealer adds an authorization of its own, signed over the enlarged set. The victim signed
/// over a set without it, so the victim's signature no longer verifies.
#[test]
fn an_authorization_added_by_the_sealer_is_refused() {
    let scenario = Scenario::new();
    let keys = [public_key(&scenario.victim), scenario.seal_signer()];
    let mut list = vec![scenario.victim_signature(), scenario.sign(&scenario.sealer, &keys)];
    list.sort_by_key(|sig| *sig.public_key());

    let err = validate(&scenario.seal(list)).unwrap_err();
    assert!(matches!(err, TransactionValidationError::InvalidSignature), "{err}");
}

/// The sealer pads the list with throwaway keys, each signed over the enlarged set. Each new key is
/// a new set, and the victim's signature verifies against none of them.
#[test]
fn authorizations_by_throwaway_keys_are_refused() {
    let scenario = Scenario::new();
    for seed in 10u64..15 {
        let throwaway = PrivateKey::from(seed);
        let keys = [public_key(&scenario.victim), public_key(&throwaway)];
        let mut list = vec![scenario.victim_signature(), scenario.sign(&throwaway, &keys)];
        list.sort_by_key(|sig| *sig.public_key());

        let err = validate(&scenario.seal(list)).unwrap_err();
        assert!(matches!(err, TransactionValidationError::InvalidSignature), "{err}");
    }
}

/// What remains to the sealer is re-sealing, or asking the victim to sign again: neither moves the
/// id, so dedup still recognises the copy.
#[test]
fn re_sealing_and_re_signing_keep_the_id() {
    let scenario = Scenario::new();
    let first = scenario.seal(vec![scenario.victim_signature()]);
    let resealed = scenario.seal(first.signatures().to_vec());
    let resigned = scenario.seal(vec![scenario.victim_signature()]);

    assert_ne!(first.signatures()[0].signature(), resigned.signatures()[0].signature());
    validate(&resigned).unwrap();
    assert_eq!(first.calculate_id(), resealed.calculate_id());
    assert_eq!(first.calculate_id(), resigned.calculate_id());
}

/// V1 is accepted under its own rules: its authorizations do not sign the authorizer set and its
/// list may come in any order, so a reordered list is a second valid transaction with its own id.
/// Only V2 closes that.
#[test]
fn v1_transactions_keep_their_rules_during_the_transition() {
    let scenario = Scenario::new();
    let other = PrivateKey::from(4u64);
    let seal_signer = scenario.seal_signer();
    let unbound = AuthorizerSet::new();
    let context = AuthorizationContext::new(&seal_signer, &unbound);
    let list = vec![
        v1_signing::sign_authorization(&scenario.victim, context, &scenario.body),
        v1_signing::sign_authorization(&other, context, &scenario.body),
    ];

    let seal_v1 = |signatures: Vec<TransactionSignature>| {
        v1_signing::seal(
            UnsealedTransactionV1::new(scenario.body.clone(), signatures),
            &scenario.sealer,
        )
    };
    let forward = seal_v1(list.clone());
    let reversed = seal_v1(list.into_iter().rev().collect());

    for tx in [&forward, &reversed] {
        assert_eq!(tx.schema(), SchemaVersion::V1);
        validate(tx).unwrap();
    }
    assert_ne!(forward.calculate_id(), reversed.calculate_id());
}

/// A V2 authorization does not verify on the V1 transaction with the same body, nor the reverse:
/// the schema is the first value both messages chain.
#[test]
fn an_authorization_does_not_cross_schemas() {
    let scenario = Scenario::new();
    let v2_signature = scenario.victim_signature();
    let as_v1 = v1_signing::seal(
        UnsealedTransactionV1::new(scenario.body.clone(), vec![v2_signature]),
        &scenario.sealer,
    );
    assert!(matches!(
        validate(&as_v1),
        Err(TransactionValidationError::InvalidSignature)
    ));

    let seal_signer = scenario.seal_signer();
    let authorizers: AuthorizerSet = [public_key(&scenario.victim)].into_iter().collect();
    let v1_signature = v1_signing::sign_authorization(
        &scenario.victim,
        AuthorizationContext::new(&seal_signer, &authorizers),
        &scenario.body,
    );
    let as_v2 = scenario.seal(vec![v1_signature]);
    assert!(matches!(
        validate(&as_v2),
        Err(TransactionValidationError::InvalidSignature)
    ));
}

/// Random sequences of everything a sealer can do without a co-signer's secret: reorder, repeat,
/// drop, swap in another of a co-signer's signatures, tamper, add authorizations of its own or of
/// throwaway keys under any key set, change the body, and re-seal. Every outcome is either refused
/// or the honest transaction's id.
#[test]
fn every_sealer_variant_is_refused_or_keeps_the_id() {
    use rand::{RngExt, SeedableRng, rngs::StdRng, seq::SliceRandom};
    use tari_ootle_common_types::signature::{components, from_components};

    let scenario = Scenario::new();
    let peer = PrivateKey::from(5u64);
    let honest_keys = [public_key(&scenario.victim), public_key(&peer)];
    let victim_signatures = [
        scenario.sign(&scenario.victim, &honest_keys),
        scenario.sign(&scenario.victim, &honest_keys),
    ];
    let peer_signature = scenario.sign(&peer, &honest_keys);
    let mut honest_list = vec![victim_signatures[0].clone(), peer_signature];
    honest_list.sort_by_key(|sig| *sig.public_key());
    let honest = scenario.seal(honest_list.clone());
    validate(&honest).unwrap();
    let honest_id = honest.calculate_id();

    let mut rng = StdRng::seed_from_u64(0x0071_e5a1);
    let (mut accepted, mut refused) = (0usize, 0usize);
    for iteration in 0..400u64 {
        let mut body = scenario.body.clone();
        let mut list = honest_list.clone();
        for _ in 0..rng.random_range(1..=4) {
            match rng.random_range(0..8) {
                0 => list.shuffle(&mut rng),
                1 if !list.is_empty() => {
                    let i = rng.random_range(0..list.len());
                    list.insert(rng.random_range(0..=list.len()), list[i].clone());
                },
                2 if !list.is_empty() => {
                    list.remove(rng.random_range(0..list.len()));
                },
                3 => {
                    let signer = if rng.random_bool(0.5) {
                        scenario.sealer.clone()
                    } else {
                        PrivateKey::from(1_000 + iteration)
                    };
                    let mut keys = vec![public_key(&signer)];
                    for key in honest_keys.iter().chain([&scenario.seal_signer()]) {
                        if rng.random_bool(0.5) {
                            keys.push(*key);
                        }
                    }
                    let authorizers: AuthorizerSet = keys.iter().collect();
                    let seal_signer = scenario.seal_signer();
                    let signature = TransactionSignature::sign(
                        &signer,
                        AuthorizationContext::new(&seal_signer, &authorizers),
                        &body,
                    );
                    list.insert(rng.random_range(0..=list.len()), signature);
                },
                4 => {
                    let other = &victim_signatures[rng.random_range(0..victim_signatures.len())];
                    for sig in &mut list {
                        if sig.public_key() == other.public_key() {
                            *sig = other.clone();
                        }
                    }
                },
                5 if list.len() >= 2 => {
                    let (i, j) = (rng.random_range(0..list.len()), rng.random_range(0..list.len()));
                    let (i_nonce, i_scalar) = components(list[i].signature()).unwrap();
                    let (j_nonce, j_scalar) = components(list[j].signature()).unwrap();
                    list[i] = TransactionSignature::new(*list[i].public_key(), from_components(&i_nonce, &j_scalar));
                    list[j] = TransactionSignature::new(*list[j].public_key(), from_components(&j_nonce, &i_scalar));
                },
                6 => {
                    list.sort_by_key(|sig| *sig.public_key());
                    list.dedup_by_key(|sig| *sig.public_key());
                },
                7 => body.nonce = body.nonce.wrapping_add(rng.random_range(1..=3)),
                _ => {},
            }
        }

        let variant = UnsealedTransactionV1::new(body, list).seal(&scenario.sealer);
        match validate(&variant) {
            Ok(()) => {
                assert_eq!(
                    variant.calculate_id(),
                    honest_id,
                    "iteration {iteration}: a valid variant with a new id"
                );
                accepted += 1;
            },
            Err(_) => refused += 1,
        }
    }
    assert!(
        accepted > 0 && refused > 0,
        "the walk must reach both outcomes to mean anything: {accepted} accepted, {refused} refused"
    );
}
