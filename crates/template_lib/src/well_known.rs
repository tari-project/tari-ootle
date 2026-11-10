//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The types a template definition tags as [`Type::WellKnown`](tari_template_abi::Type::WellKnown).
//!
//! The `#[template]` macro sees only a type's name. When the name is a [`WellKnownType`], the macro tags it and
//! emits a call to [`assert_well_known`] in the template's scope, so the template fails to build if the name
//! resolves to some other type. [`WellKnown`] is sealed: implementing it for another type would let that type pass
//! the check.

pub use tari_template_abi::WellKnownType;
use tari_template_lib_types::{
    AccessRule,
    Amount,
    AuthHookCaller,
    ComponentAddress,
    Hash32,
    Metadata,
    NonFungibleAddress,
    NonFungibleId,
    OwnerRule,
    ResourceAddress,
    ResourceType,
    UtxoAddress,
    VaultId,
    access_rules::ResourceAuthAction,
    bytes::Bytes,
    confidential::ConfidentialWithdrawProof,
    crypto::RistrettoPublicKeyBytes,
};

use crate::{
    SpendContext,
    models::{Bucket, Proof},
};

mod sealed {
    pub trait Sealed {}
}

/// A `tari_template_lib` type that a template definition names by [`WellKnownType`].
#[diagnostic::on_unimplemented(
    message = "`{Self}` has the name of a well-known template type, but is not that type",
    note = "rename the type, because a template definition names `{Self}` as the `tari_template_lib` type"
)]
pub trait WellKnown: sealed::Sealed {
    const KIND: WellKnownType;
}

/// Compiles only when `T` is the type that `tag` names.
pub const fn assert_well_known<T: WellKnown>(tag: WellKnownType) {
    let expected = match tag {
        WellKnownType::TemplateAddress => WellKnownType::Hash32,
        tag => tag,
    };
    assert!(
        T::KIND as u8 == expected as u8,
        "the type has the name of a different well-known template type"
    );
}

macro_rules! impl_well_known {
    ($($ty:ty => $kind:ident,)+) => {
        $(
            impl sealed::Sealed for $ty {}
            impl WellKnown for $ty {
                const KIND: WellKnownType = WellKnownType::$kind;
            }
        )+

        #[cfg(test)]
        const IMPLEMENTED: &[WellKnownType] = &[$(<$ty as WellKnown>::KIND),+];
    };
}

impl_well_known! {
    Amount => Amount,
    Bucket => Bucket,
    Proof => Proof,
    ComponentAddress => ComponentAddress,
    ResourceAddress => ResourceAddress,
    VaultId => VaultId,
    NonFungibleId => NonFungibleId,
    NonFungibleAddress => NonFungibleAddress,
    UtxoAddress => UtxoAddress,
    Hash32 => Hash32,
    Metadata => Metadata,
    Bytes => Bytes,
    RistrettoPublicKeyBytes => RistrettoPublicKeyBytes,
    ResourceType => ResourceType,
    OwnerRule => OwnerRule,
    AccessRule => AccessRule,
    ConfidentialWithdrawProof => ConfidentialWithdrawProof,
    ResourceAuthAction => ResourceAuthAction,
    AuthHookCaller => AuthHookCaller,
    SpendContext => SpendContext,
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every [`WellKnownType`] has a `tari_template_lib` type, except the alias that shares one.
    #[test]
    fn every_well_known_type_has_an_implementation() {
        for kind in WellKnownType::ALL {
            assert_eq!(
                IMPLEMENTED.contains(kind),
                *kind != WellKnownType::TemplateAddress,
                "{kind:?}"
            );
        }
    }
}
