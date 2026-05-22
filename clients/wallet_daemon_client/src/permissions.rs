//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{
    collections::HashSet,
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use tari_engine_types::substate::SubstateId;
use tari_template_lib_types::{ComponentAddress, ResourceAddress};

#[derive(Debug, Clone, Deserialize, Serialize, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum JrpcPermission {
    AccountInfo,
    NftGetOwnershipProof(Option<ResourceAddress>),
    AccountBalance(SubstateId),
    AccountList(Option<ComponentAddress>),
    SubstatesRead,
    TemplatesRead,
    KeyList,
    TransactionGet,
    TransactionSend(Option<SubstateId>),
    // This can't be set via cli, after we agree on the permissions I can add the from_str.
    GetNft(Option<SubstateId>, Option<ResourceAddress>),
    // User should never grant this permission, it will be generated only by the UI to start the webrtc session.
    StartWebrtc,
    AddressBook(AddressBookPermission),
    Admin,
}

#[derive(Debug, thiserror::Error)]
#[error("Invalid permissions '{0}'")]
pub struct InvalidJrpcPermissionsFormat(String);

impl FromStr for JrpcPermission {
    type Err = InvalidJrpcPermissionsFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        // First the empty and optional
        match s.split_once('_') {
            Some(("NftGetOwnershipProof", addr)) => Ok(JrpcPermission::NftGetOwnershipProof(Some(
                ResourceAddress::from_str(addr).map_err(|e| InvalidJrpcPermissionsFormat(e.to_string()))?,
            ))),
            Some(("AccountBalance", addr)) => Ok(JrpcPermission::AccountBalance(
                SubstateId::from_str(addr).map_err(|e| InvalidJrpcPermissionsFormat(e.to_string()))?,
            )),
            Some(("AccountList", addr)) => Ok(JrpcPermission::AccountList(Some(
                ComponentAddress::from_str(addr).map_err(|e| InvalidJrpcPermissionsFormat(e.to_string()))?,
            ))),
            Some(("TransactionSend", addr)) => Ok(JrpcPermission::TransactionSend(Some(
                SubstateId::from_str(addr).map_err(|e| InvalidJrpcPermissionsFormat(e.to_string()))?,
            ))),
            Some(("AddressBook", perm)) => Ok(JrpcPermission::AddressBook(perm.parse()?)),
            Some(_) => Err(InvalidJrpcPermissionsFormat(s.to_string())),
            None => match s {
                "AccountInfo" => Ok(JrpcPermission::AccountInfo),
                "NftGetOwnershipProof" => Ok(JrpcPermission::NftGetOwnershipProof(None)),
                "AccountList" => Ok(JrpcPermission::AccountList(None)),
                "SubstatesRead" => Ok(JrpcPermission::SubstatesRead),
                "TemplatesRead" => Ok(JrpcPermission::TemplatesRead),
                "KeyList" => Ok(JrpcPermission::KeyList),
                "GetNft" => Ok(JrpcPermission::GetNft(None, None)),
                "TransactionGet" => Ok(JrpcPermission::TransactionGet),
                "TransactionSend" => Ok(JrpcPermission::TransactionSend(None)),
                "StartWebrtc" => Ok(JrpcPermission::StartWebrtc),
                "Admin" => Ok(JrpcPermission::Admin),
                _ => Err(InvalidJrpcPermissionsFormat(s.to_string())),
            },
        }
    }
}

impl Display for JrpcPermission {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            JrpcPermission::AccountInfo => write!(f, "AccountInfo"),
            JrpcPermission::NftGetOwnershipProof(Some(a)) => write!(f, "NftGetOwnershipProof_{}", a),
            JrpcPermission::NftGetOwnershipProof(None) => write!(f, "NftGetOwnershipProof"),
            JrpcPermission::AccountBalance(a) => write!(f, "AccountBalance_{}", a),
            JrpcPermission::AccountList(None) => write!(f, "AccountList"),
            JrpcPermission::AccountList(Some(a)) => write!(f, "AccountList_{}", a),
            JrpcPermission::KeyList => write!(f, "KeyList"),
            JrpcPermission::TransactionGet => write!(f, "TransactionGet"),
            JrpcPermission::TransactionSend(None) => write!(f, "TransactionSend"),
            JrpcPermission::TransactionSend(Some(s)) => write!(f, "TransactionSend_{}", s),
            JrpcPermission::GetNft(_, _) => write!(f, "GetNft"),
            JrpcPermission::StartWebrtc => write!(f, "StartWebrtc"),
            JrpcPermission::Admin => write!(f, "Admin"),
            JrpcPermission::SubstatesRead => write!(f, "SubstatesRead"),
            JrpcPermission::TemplatesRead => write!(f, "TemplatesRead"),
            JrpcPermission::AddressBook(perm) => write!(f, "AddressBook({})", perm),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct JrpcPermissions(HashSet<JrpcPermission>);

impl FromStr for JrpcPermissions {
    type Err = InvalidJrpcPermissionsFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(JrpcPermissions(
            s.split(',')
                .map(|s| s.trim())
                .map(JrpcPermission::from_str)
                .collect::<Result<_, _>>()?,
        ))
    }
}

impl JrpcPermissions {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    pub fn len(&self) -> usize {
        self.0.len()
    }

    pub fn has_permission(&self, permission: &JrpcPermission) -> bool {
        self.0.contains(permission)
    }

    /// Borrow-iterate the granted permissions without consuming or
    /// cloning the set. Useful for serialisation paths that just need
    /// to render each permission to a string — avoids the
    /// `clone().into_vec().iter()` triple-dance.
    pub fn iter(&self) -> impl Iterator<Item = &JrpcPermission> {
        self.0.iter()
    }

    pub fn into_vec(self) -> Vec<JrpcPermission> {
        self.0.into_iter().collect()
    }
}

impl TryFrom<&[String]> for JrpcPermissions {
    type Error = InvalidJrpcPermissionsFormat;

    fn try_from(value: &[String]) -> Result<Self, Self::Error> {
        let mut permissions = HashSet::with_capacity(value.len());
        for permission in value {
            permissions.insert(JrpcPermission::from_str(permission)?);
        }
        Ok(JrpcPermissions(permissions))
    }
}

impl From<Vec<JrpcPermission>> for JrpcPermissions {
    fn from(value: Vec<JrpcPermission>) -> Self {
        JrpcPermissions(value.into_iter().collect())
    }
}

impl FromIterator<JrpcPermission> for JrpcPermissions {
    fn from_iter<T: IntoIterator<Item = JrpcPermission>>(iter: T) -> Self {
        JrpcPermissions(iter.into_iter().collect())
    }
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct Claims {
    pub permissions: JrpcPermissions,
    pub exp: u64,
}

#[derive(Debug, Clone, Deserialize, Serialize, Hash, Eq, PartialEq)]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[repr(u8)]
pub enum AddressBookPermission {
    Read = 0x01,
    Create = 0x10,
    Update = 0x20,
    Delete = 0x80,
}

impl FromStr for AddressBookPermission {
    type Err = InvalidJrpcPermissionsFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_lowercase().as_str() {
            "read" => Ok(AddressBookPermission::Read),
            "create" => Ok(AddressBookPermission::Create),
            "update" => Ok(AddressBookPermission::Update),
            "delete" => Ok(AddressBookPermission::Delete),
            _ => Err(InvalidJrpcPermissionsFormat(s.to_string())),
        }
    }
}

impl Display for AddressBookPermission {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        match self {
            AddressBookPermission::Read => write!(f, "read"),
            AddressBookPermission::Create => write!(f, "create"),
            AddressBookPermission::Update => write!(f, "update"),
            AddressBookPermission::Delete => write!(f, "delete"),
        }
    }
}
