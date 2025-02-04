// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

use base64urlsafedata::Base64UrlSafeData;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// <https://w3c.github.io/webauthn/#dictionary-makecredentialoptions>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct PublicKeyCredentialCreationOptions {
    /// The relying party
    pub rp: RelyingParty,
    /// The user.
    pub user: User,
    /// The one-time challenge for the credential to sign.
    pub challenge: Vec<u8>,
    /// The set of cryptographic types allowed by this server.
    pub pub_key_cred_params: Vec<PubKeyCredParams>,

    /// The timeout for the authenticator to stop accepting the operation
    #[serde(skip_serializing_if = "Option::is_none")]
    pub timeout: Option<u32>,

    /// Credential ID's that are excluded from being able to be registered.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub exclude_credentials: Option<Vec<PublicKeyCredentialDescriptor>>,

    /// Criteria defining which authenticators may be used in this operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authenticator_selection: Option<AuthenticatorSelectionCriteria>,

    /// Hints defining which credentials may be used in this operation.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hints: Option<Vec<PublicKeyCredentialHints>>,

    /// The requested attestation level from the device.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation: Option<AttestationConveyancePreference>,

    /// The list of attestation formats that the RP will accept.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub attestation_formats: Option<Vec<AttestationFormat>>,

    /// Non-standard extensions that may be used by the browser/authenticator.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub extensions: Option<RequestRegistrationExtensions>,
}

/// Relying Party Entity
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct RelyingParty {
    /// The name of the relying party.
    pub name: String,
    /// The id of the relying party.
    pub id: String,
    // Note: "icon" is deprecated: https://github.com/w3c/webauthn/pull/1337
}

/// User Entity
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct User {
    /// The user's id in base64 form. This MUST be a unique id, and
    /// must NOT contain personally identifying information, as this value can NEVER
    /// be changed. If in doubt, use a UUID.
    pub id: Vec<u8>,
    /// A detailed name for the account, such as an email address. This value
    /// **can** change, so **must not** be used as a primary key.
    pub name: String,
    /// The user's preferred name for display. This value **can** change, so
    /// **must not** be used as a primary key.
    pub display_name: String,
    // Note: "icon" is deprecated: https://github.com/w3c/webauthn/pull/1337
}

/// Public key cryptographic parameters
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize)]
pub struct PubKeyCredParams {
    /// The type of public-key credential.
    #[serde(rename = "type")]
    pub type_: String,
    /// The algorithm in use defined by COSE.
    pub alg: i64,
}

/// <https://www.w3.org/TR/webauthn/#dictdef-publickeycredentialdescriptor>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize, PartialEq, Eq)]
pub struct PublicKeyCredentialDescriptor {
    /// The type of credential
    #[serde(rename = "type")]
    pub type_: String,
    /// The credential id.
    pub id: Vec<u8>,
    /// The allowed transports for this credential. Note this is a hint, and is NOT
    /// enforced.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transports: Option<Vec<AuthenticatorTransport>>,
}

/// <https://www.w3.org/TR/webauthn/#enumdef-authenticatortransport>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[allow(unused)]
pub enum AuthenticatorTransport {
    /// <https://www.w3.org/TR/webauthn/#dom-authenticatortransport-usb>
    Usb,
    /// <https://www.w3.org/TR/webauthn/#dom-authenticatortransport-nfc>
    Nfc,
    /// <https://www.w3.org/TR/webauthn/#dom-authenticatortransport-ble>
    Ble,
    /// <https://www.w3.org/TR/webauthn/#dom-authenticatortransport-internal>
    Internal,
    /// Hybrid transport, formerly caBLE. Part of the level 3 draft specification.
    /// <https://w3c.github.io/webauthn/#dom-authenticatortransport-hybrid>
    Hybrid,
    /// Test transport; used for Windows 10.
    Test,
    /// An unknown transport was provided - it will be ignored.
    #[serde(other)]
    Unknown,
}

/// <https://www.w3.org/TR/webauthn/#dictdef-authenticatorselectioncriteria>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Default, Serialize, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct AuthenticatorSelectionCriteria {
    /// How the authenticator should be attached to the client machine.
    /// Note this is only a hint. It is not enforced in anyway shape or form.
    /// <https://www.w3.org/TR/webauthn/#attachment>
    #[serde(skip_serializing_if = "Option::is_none")]
    pub authenticator_attachment: Option<AuthenticatorAttachment>,

    /// Hint to the credential to create a resident key. Note this value should be
    /// a member of ResidentKeyRequirement, but client must ignore unknown values,
    /// treating an unknown value as if the member does not exist.
    /// <https://www.w3.org/TR/webauthn-2/#dom-authenticatorselectioncriteria-residentkey>
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resident_key: Option<ResidentKeyRequirement>,

    /// Hint to the credential to create a resident key. Note this can not be enforced
    /// or validated, so the authenticator may choose to ignore this parameter.
    /// <https://www.w3.org/TR/webauthn/#resident-credential>
    pub require_resident_key: bool,

    /// The user verification level to request during registration. Depending on if this
    /// authenticator provides verification may affect future interactions as this is
    /// associated to the credential during registration.
    pub user_verification: UserVerificationPolicy,
}

/// The authenticator attachment hint. This is NOT enforced, and is only used
/// to help a user select a relevant authenticator type.
///
/// <https://www.w3.org/TR/webauthn/#attachment>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Copy, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum AuthenticatorAttachment {
    /// Request a device that is part of the machine aka inseperable.
    /// <https://www.w3.org/TR/webauthn/#attachment>
    #[serde(rename = "platform")]
    Platform,
    /// Request a device that can be seperated from the machine aka an external token.
    /// <https://www.w3.org/TR/webauthn/#attachment>
    #[serde(rename = "cross-platform")]
    CrossPlatform,
}

/// The Relying Party's requirements for client-side discoverable credentials.
///
/// <https://www.w3.org/TR/webauthn-2/#enumdef-residentkeyrequirement>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Clone, Copy, Debug, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[serde(rename_all = "lowercase")]
pub enum ResidentKeyRequirement {
    /// <https://www.w3.org/TR/webauthn-2/#dom-residentkeyrequirement-discouraged>
    Discouraged,
    /// ⚠️  In all major browsers preferred is identical in behaviour to required.
    /// You should use required instead.
    /// <https://www.w3.org/TR/webauthn-2/#dom-residentkeyrequirement-preferred>
    Preferred,
    /// <https://www.w3.org/TR/webauthn-2/#dom-residentkeyrequirement-required>
    Required,
}

/// Defines the User Authenticator Verification policy. This is documented
/// <https://w3c.github.io/webauthn/#enumdef-userverificationrequirement>, and each
/// variant lists it's effects.
///
/// To be clear, Verification means that the Authenticator perform extra or supplementary
/// interaction with the user to verify who they are. An example of this is Apple Touch Id
/// required a fingerprint to be verified, or a yubico device requiring a pin in addition to
/// a touch event.
///
/// An example of a non-verified interaction is a yubico device with no pin where touch is
/// the only interaction - we only verify a user is present, but we don't have extra details
/// to the legitimacy of that user.
///
/// As UserVerificationPolicy is *only* used in credential registration, this stores the
/// verification state of the credential in the persisted credential. These persisted
/// credentials define which UserVerificationPolicy is issued during authentications.
///
/// **IMPORTANT** - Due to limitations of the webauthn specification, CTAP devices, and browser
/// implementations, the only secure choice as an RP is *required*.
///
/// > ⚠️  **WARNING** - discouraged is marked with a warning, as some authenticators
/// > will FORCE verification during registration but NOT during authentication.
/// > This makes it impossible for a relying party to *consistently* enforce user verification,
/// > which can confuse users and lead them to distrust user verification is being enforced.
///
/// > ⚠️  **WARNING** - preferred can lead to authentication errors in some cases due to browser
/// > peripheral exchange allowing authentication verification bypass. Webauthn RS is not vulnerable
/// > to these bypasses due to our
/// > tracking of UV during registration through authentication, however preferred can cause
/// > legitimate credentials to not prompt for UV correctly due to browser perhipheral exchange
/// > leading Webauthn RS to deny them in what should otherwise be legitimate operations.
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Clone, Copy, Debug, Default, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
#[allow(non_camel_case_types)]
#[serde(rename_all = "lowercase")]
pub enum UserVerificationPolicy {
    /// Require user verification bit to be set, and fail the registration or authentication
    /// if false. If the authenticator is not able to perform verification, it will not be
    /// usable with this policy.
    ///
    /// This policy is the default as it is the only secure and consistent user verification option.
    #[serde(rename = "required")]
    #[default]
    Required,
    /// Prefer UV if possible, but ignore if not present. In other webauthn deployments this is bypassable
    /// as it implies the library will not check UV is set correctly for this credential. Webauthn-RS
    /// is *not* vulnerable to this as we check the UV state always based on it's presence at registration.
    ///
    /// However, in some cases use of this policy can lead to some credentials failing to verify
    /// correctly due to browser peripheral exchange bypasses.
    #[serde(rename = "preferred")]
    Preferred,
    /// Discourage - but do not prevent - user verification from being supplied. Many CTAP devices
    /// will attempt UV during registration but not authentication leading to user confusion.
    #[serde(rename = "discouraged")]
    Discouraged_DO_NOT_USE,
}

/// A hint as to the class of device that is expected to fufil this operation.
///
/// <https://www.w3.org/TR/webauthn-3/#enumdef-publickeycredentialhints>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
#[allow(unused)]
pub enum PublicKeyCredentialHints {
    /// The credential is a removable security key
    SecurityKey,
    /// The credential is a platform authenticator
    ClientDevice,
    /// The credential will come from an external device
    Hybrid,
}

/// <https://www.w3.org/TR/webauthn/#enumdef-attestationconveyancepreference>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AttestationConveyancePreference {
    /// Do not request attestation.
    /// <https://www.w3.org/TR/webauthn/#dom-attestationconveyancepreference-none>
    #[default]
    None,

    /// Request attestation in a semi-anonymized form.
    /// <https://www.w3.org/TR/webauthn/#dom-attestationconveyancepreference-indirect>
    Indirect,

    /// Request attestation in a direct form.
    /// <https://www.w3.org/TR/webauthn/#dom-attestationconveyancepreference-direct>
    Direct,
}

/// The type of attestation on the credential
///
/// <https://www.iana.org/assignments/webauthn/webauthn.xhtml>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Hash)]
pub enum AttestationFormat {
    /// Packed attestation
    #[serde(rename = "packed", alias = "Packed")]
    Packed,
    /// TPM attestation (like Microsoft)
    #[serde(rename = "tpm", alias = "Tpm", alias = "TPM")]
    Tpm,
    /// Android hardware attestation
    #[serde(rename = "android-key", alias = "AndroidKey")]
    AndroidKey,
    /// Older Android Safety Net
    #[serde(
        rename = "android-safetynet",
        alias = "AndroidSafetyNet",
        alias = "AndroidSafetynet"
    )]
    AndroidSafetyNet,
    /// Old U2F attestation type
    #[serde(rename = "fido-u2f", alias = "FIDOU2F")]
    FIDOU2F,
    /// Apple touchID/faceID
    #[serde(rename = "apple", alias = "AppleAnonymous")]
    AppleAnonymous,
    /// No attestation
    #[serde(rename = "none", alias = "None")]
    None,
}

/// Extension option inputs for PublicKeyCredentialCreationOptions.
///
/// Implements \[AuthenticatorExtensionsClientInputs\] from the spec.
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RequestRegistrationExtensions {
    /// The `credProtect` extension options
    #[serde(flatten, skip_serializing_if = "Option::is_none")]
    pub cred_protect: Option<CredProtect>,

    /// ⚠️  - Browsers do not support this!
    /// Uvm
    #[serde(skip_serializing_if = "Option::is_none")]
    pub uvm: Option<bool>,

    /// ⚠️  - This extension result is always unsigned, and only indicates if the
    /// browser *requests* a residentKey to be created. It has no bearing on the
    /// true rk state of the credential.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub cred_props: Option<bool>,

    /// CTAP2.1 Minumum pin length
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_pin_length: Option<bool>,

    /// ⚠️  - Browsers support the *creation* of the secret, but not the retrieval of it.
    /// CTAP2.1 create hmac secret
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hmac_create_secret: Option<bool>,
}

/// The desired options for the client's use of the `credProtect` extension
///
/// <https://fidoalliance.org/specs/fido-v2.1-rd-20210309/fido-client-to-authenticator-protocol-v2.1-rd-20210309.html#sctn-credProtect-extension>
#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct CredProtect {
    /// The credential policy to enact
    pub credential_protection_policy: CredentialProtectionPolicy,
    /// Whether it is better for the authenticator to fail to create a
    /// credential rather than ignore the protection policy
    /// If no value is provided, the client treats it as `false`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub enforce_credential_protection_policy: Option<bool>,
}

#[cfg_attr(
    feature = "ts",
    derive(TS),
    ts(export, export_to = "../../bindings/src/types/wallet-daemon-client/")
)]
#[derive(Debug, Serialize, Clone, Copy, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
#[repr(u8)]
pub enum CredentialProtectionPolicy {
    /// This reflects "FIDO_2_0" semantics. In this configuration, performing
    /// some form of user verification is optional with or without credentialID
    /// list. This is the default state of the credential if the extension is
    /// not specified.
    UserVerificationOptional = 0x1,
    /// In this configuration, credential is discovered only when its
    /// credentialID is provided by the platform or when some form of user
    /// verification is performed.
    UserVerificationOptionalWithCredentialIDList = 0x2,
    /// This reflects that discovery and usage of the credential MUST be
    /// preceded by some form of user verification.
    UserVerificationRequired = 0x3,
}

// TODO: implement separately From<...> impls for specific types

impl From<PublicKeyCredentialCreationOptions> for webauthn_rs_proto::PublicKeyCredentialCreationOptions {
    fn from(value: PublicKeyCredentialCreationOptions) -> Self {
        Self {
            rp: webauthn_rs_proto::RelyingParty { 
                name: value.rp.name, 
                id: value.rp.id,
            },
            user: webauthn_rs_proto::User {
                id: Base64UrlSafeData::from(value.user.id),
                name: value.user.name,
                display_name: value.user.display_name,
            },
            challenge: Base64UrlSafeData::from(value.challenge),
            pub_key_cred_params: value.pub_key_cred_params.iter().map(|param| {
                webauthn_rs_proto::PubKeyCredParams{ type_: param.type_.clone(), alg: param.alg }
            }).collect(),
            timeout: value.timeout,
            exclude_credentials: value.exclude_credentials.map(|creds| {
                creds.iter().map(|descriptor| {
                    webauthn_rs_proto::PublicKeyCredentialDescriptor{
                        type_: descriptor.type_.clone(),
                        id: Base64UrlSafeData::from(descriptor.id.clone()),
                        transports: descriptor.transports.clone().map(|transports| {
                            transports.iter().map(|transport| {
                                match transport {
                                    AuthenticatorTransport::Usb => webauthn_rs_proto::AuthenticatorTransport::Usb,
                                    AuthenticatorTransport::Nfc => webauthn_rs_proto::AuthenticatorTransport::Nfc,
                                    AuthenticatorTransport::Ble => webauthn_rs_proto::AuthenticatorTransport::Ble,
                                    AuthenticatorTransport::Internal => webauthn_rs_proto::AuthenticatorTransport::Internal,
                                    AuthenticatorTransport::Hybrid => webauthn_rs_proto::AuthenticatorTransport::Hybrid,
                                    AuthenticatorTransport::Test => webauthn_rs_proto::AuthenticatorTransport::Test,
                                    AuthenticatorTransport::Unknown => webauthn_rs_proto::AuthenticatorTransport::Unknown,
                                }
                            })
                                .collect()
                        }),
                    }
                }).collect()
            }),
            authenticator_selection: None,
            hints: None,
            attestation: None,
            attestation_formats: None,
            extensions: None,
        }
    }
}