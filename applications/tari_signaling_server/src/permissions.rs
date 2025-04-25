//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

// TODO: This is a temporary solution to allow the code to compile - we will remove the signalling server in the near
// future

use std::{
    fmt::{Display, Formatter},
    str::FromStr,
};

use serde::{Deserialize, Serialize};
use tari_engine_types::{
    substate::SubstateId,
    template_lib_models::{ComponentAddress, ResourceAddress},
};

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
            JrpcPermission::AccountInfo => f.write_str("AccountInfo"),
            JrpcPermission::NftGetOwnershipProof(Some(a)) => f.write_str(&format!("NftGetOwnershipProof_{}", a)),
            JrpcPermission::NftGetOwnershipProof(None) => f.write_str("NftGetOwnershipProof"),
            JrpcPermission::AccountBalance(a) => f.write_str(&format!("AccountBalance_{}", a)),
            JrpcPermission::AccountList(None) => f.write_str("AccountList"),
            JrpcPermission::AccountList(Some(a)) => f.write_str(&format!("AccountList_{}", a)),
            JrpcPermission::KeyList => f.write_str("KeyList"),
            JrpcPermission::TransactionGet => f.write_str("TransactionGet"),
            JrpcPermission::TransactionSend(None) => f.write_str("TransactionSend"),
            JrpcPermission::TransactionSend(Some(s)) => f.write_str(&format!("TransactionSend_{}", s)),
            JrpcPermission::GetNft(_, _) => f.write_str("GetNft"),
            JrpcPermission::StartWebrtc => f.write_str("StartWebrtc"),
            JrpcPermission::Admin => f.write_str("Admin"),
            JrpcPermission::SubstatesRead => f.write_str("SubstatesRead"),
            JrpcPermission::TemplatesRead => f.write_str("TemplatesRead"),
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JrpcPermissions(pub Vec<JrpcPermission>);

impl FromStr for JrpcPermissions {
    type Err = InvalidJrpcPermissionsFormat;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Ok(JrpcPermissions(
            s.split(',').map(JrpcPermission::from_str).collect::<Result<_, _>>()?,
        ))
    }
}

impl TryFrom<&[String]> for JrpcPermissions {
    type Error = InvalidJrpcPermissionsFormat;

    fn try_from(value: &[String]) -> Result<Self, Self::Error> {
        let mut permissions = Vec::new();
        for permission in value {
            permissions.push(JrpcPermission::from_str(permission)?);
        }
        Ok(JrpcPermissions(permissions))
    }
}

#[derive(Debug, Clone, Deserialize, Serialize, Eq, PartialEq)]
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
    Admin,
}
