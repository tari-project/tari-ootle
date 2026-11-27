//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_engine_types::published_template::{PublishedTemplate, PublishedTemplateAddress};
use tari_ootle_common_types::{
    SubstateAddress,
    SubstateVersion,
    optional::Optional,
    services::template_provider::TemplateProvider,
};
use tari_ootle_storage::StorageError;
use tari_template_lib_types::TemplateAddress;

use crate::{
    RocksDbStateStore,
    codecs::{DbDecoder, DefaultCodec},
    column_families::substate::SubstateCf,
};

#[derive(minicbor::Decode)]
enum VersionedSubstateLiveness {
    #[n(0)]
    V1(#[n(0)] SubstateLiveness),
}

#[derive(minicbor::Decode)]
struct SubstateLiveness {
    #[n(2)]
    substate_value: Option<PresentValue>,
    #[n(5)]
    destroyed: Option<PresentValue>,
}

struct PresentValue;

impl<'b, C> minicbor::Decode<'b, C> for PresentValue {
    fn decode(d: &mut minicbor::Decoder<'b>, _ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        d.skip()?;
        Ok(Self)
    }
}

impl<TAddr: Send + Sync + 'static> TemplateProvider for RocksDbStateStore<TAddr> {
    type Error = StorageError;
    type Template = PublishedTemplate;

    fn get_template(&self, id: &TemplateAddress) -> Result<Option<Self::Template>, Self::Error> {
        const OPERATION: &str = "RocksDbReadOnlyStateStore::get_template";
        let tx = self.read_view();
        let cf = tx.db().cf(SubstateCf)?;
        let address = template_address_to_substate_address(*id);
        let Some(substate) = cf.get(&address, OPERATION).optional()? else {
            return Ok(None);
        };
        if substate.is_destroyed() {
            return Ok(None);
        }
        let Some(value) = substate.into_substate_value() else {
            return Ok(None);
        };

        let template = value.into_template().ok_or_else(|| StorageError::DataInconsistency {
            details: format!("Template substate does not contain a published template: {}", address),
        })?;

        Ok(Some(template))
    }

    fn has_template(&self, id: &TemplateAddress) -> Result<bool, Self::Error> {
        const OPERATION: &str = "RocksDbReadOnlyStateStore::has_template";
        let tx = self.read_view();
        let cf = tx.db().cf(SubstateCf)?;
        let address = template_address_to_substate_address(*id);
        let Some(bytes) = cf.get_raw_pinned(&address, OPERATION)? else {
            return Ok(false);
        };
        let VersionedSubstateLiveness::V1(liveness) =
            DefaultCodec::<VersionedSubstateLiveness>::new().decode_exact(&bytes)?;
        Ok(liveness.substate_value.is_some() && liveness.destroyed.is_none())
    }
}

fn template_address_to_substate_address(address: TemplateAddress) -> SubstateAddress {
    let address = PublishedTemplateAddress::from_hash(address);
    SubstateAddress::from_object_key(address.as_object_key(), SubstateVersion::ZERO)
}
