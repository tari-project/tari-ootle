//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::prelude::*;

#[cfg(feature = "lookalike")]
mod lookalike {
    use tari_template_lib::template_macro_deps::minicbor;

    #[derive(minicbor::Encode, minicbor::Decode, minicbor::CborLen)]
    pub struct Bucket {}
}

#[template]
mod well_known_types {
    use super::*;
    #[cfg(feature = "lookalike")]
    use crate::lookalike::Bucket;

    pub struct WellKnownTypes {}

    impl WellKnownTypes {
        pub fn take(
            bucket: Bucket,
            amounts: Vec<Amount>,
            template: Option<TemplateAddress>,
            owner: (ComponentAddress, NonFungibleId),
        ) -> Amount {
            let _unused = (bucket, amounts, template, owner);
            Amount::zero()
        }
    }
}
