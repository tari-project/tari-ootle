//   Copyright 2022 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    fmt::{Debug, Display},
    hash::Hash,
};

use libp2p_identity::PeerId;
use ootle_byte_type::ToByteType;
use serde::{de::DeserializeOwned, Serialize};
use tari_crypto::ristretto::RistrettoPublicKey;
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

pub trait NodeAddressable:
    Eq + Hash + Clone + Debug + Ord + Send + Sync + Display + Serialize + DeserializeOwned
{
    fn zero() -> Self;

    fn try_from_public_key(_: &RistrettoPublicKey) -> Option<Self> {
        None
    }
}

impl NodeAddressable for String {
    fn zero() -> Self {
        "".to_string()
    }

    fn try_from_public_key(_: &RistrettoPublicKey) -> Option<Self> {
        None
    }
}

impl NodeAddressable for RistrettoPublicKeyBytes {
    fn zero() -> Self {
        RistrettoPublicKeyBytes::default()
    }

    fn try_from_public_key(public_key: &RistrettoPublicKey) -> Option<Self> {
        Some(public_key.to_byte_type())
    }
}

pub trait DerivableFromPublicKey: NodeAddressable {
    fn derive_from_public_key(public_key: &RistrettoPublicKey) -> Self {
        Self::try_from_public_key(public_key)
            .expect("Marker trait DerivableFromPublicKey must always return Some from try_from_public_key")
    }

    fn eq_to_public_key(&self, public_key: &RistrettoPublicKey) -> bool {
        *self == Self::derive_from_public_key(public_key)
    }
}

impl DerivableFromPublicKey for RistrettoPublicKeyBytes {}

pub trait ToPeerId {
    fn to_peer_id(&self) -> PeerId;
}
