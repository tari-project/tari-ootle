//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fmt, fmt::Display, str::FromStr};

use crate::TariSwarmError;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ProtocolVersion {
    domain: String,
    network: String,
    version: Version,
}

impl ProtocolVersion {
    pub const fn new(domain: String, network: String, version: Version) -> Self {
        Self {
            domain,
            network,
            version,
        }
    }

    pub fn domain(&self) -> &str {
        &self.domain
    }

    pub fn network(&self) -> &str {
        &self.network
    }

    pub const fn version(&self) -> Version {
        self.version
    }

    pub fn is_compatible(&self, protocol_str: &str) -> bool {
        let Some((domain, network, version)) = parse_protocol_str(protocol_str) else {
            return false;
        };
        self.domain == domain && self.network == network && self.version.semantic_version_eq(&version)
    }
}

impl PartialEq<String> for ProtocolVersion {
    fn eq(&self, other: &String) -> bool {
        let Some((domain, network, version)) = parse_protocol_str(other) else {
            return false;
        };
        self.domain == domain && self.network == network && self.version == version
    }
}

impl Display for ProtocolVersion {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "/{}/{}/{}", self.domain, self.network, self.version)
    }
}

impl FromStr for ProtocolVersion {
    type Err = TariSwarmError;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let Some((domain, network, version)) = parse_protocol_str(value) else {
            return Err(TariSwarmError::ProtocolVersionParseFailed {
                given: value.to_string(),
            });
        };
        Ok(Self::new(domain.to_string(), network.to_string(), version))
    }
}

fn parse_protocol_str(protocol_str: &str) -> Option<(&str, &str, Version)> {
    let mut parts = protocol_str.split('/');
    // Must have a leading '/'
    let leading = parts.next()?;
    if !leading.is_empty() {
        return None;
    }

    let domain = parts.next().filter(|s| !s.is_empty())?;
    let network = parts.next().filter(|s| !s.is_empty())?;
    let version = parts.next().and_then(|s| s.parse().ok())?;
    if parts.next().is_some() {
        return None;
    }
    Some((domain, network, version))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Version {
    major: u16,
    minor: u16,
    patch: u16,
}

impl Version {
    pub const fn new(major: u16, minor: u16, patch: u16) -> Self {
        Self { major, minor, patch }
    }

    pub const fn major(&self) -> u16 {
        self.major
    }

    pub const fn minor(&self) -> u16 {
        self.minor
    }

    pub const fn patch(&self) -> u16 {
        self.patch
    }

    pub const fn semantic_version_eq(&self, other: &Version) -> bool {
        if self.major != other.major {
            return false;
        }
        // Similar to https://doc.rust-lang.org/cargo/reference/specifying-dependencies.html#specifying-dependencies-from-cratesio
        // 0.x.y any change to x is not compatible
        if self.major == 0 {
            if self.minor != other.minor {
                return false;
            }
            // 0.0.x any change to x is not compatible
            if self.minor == 0 {
                return self.patch == other.patch;
            }
            return true;
        }

        // x.y.z any change to x is not compatible
        true
    }
}

impl FromStr for Version {
    type Err = TariSwarmError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.split('.');

        let mut next = || {
            parts
                .next()
                .ok_or(TariSwarmError::InvalidVersionString { given: s.to_string() })?
                .parse()
                .map_err(|_| TariSwarmError::InvalidVersionString { given: s.to_string() })
        };
        let version = Self {
            major: next()?,
            minor: next()?,
            patch: next()?,
        };
        if parts.next().is_some() {
            return Err(TariSwarmError::InvalidVersionString { given: s.to_string() });
        }
        Ok(version)
    }
}

impl Display for Version {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}.{}.{}", self.major, self.minor, self.patch)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_parses_correctly() {
        let version = ProtocolVersion::from_str("/tari/igor/1.2.3").unwrap();
        assert_eq!(version.domain(), "tari");
        assert_eq!(version.network(), "igor");
        assert_eq!(version.version(), Version {
            major: 1,
            minor: 2,
            patch: 3
        });
    }

    #[test]
    fn it_rejects_malformed_protocol_and_version_strings() {
        assert!(ProtocolVersion::from_str("tari/igor/1.2.3").is_err());
        assert!(ProtocolVersion::from_str("//igor/1.2.3").is_err());
        assert!(ProtocolVersion::from_str("/tari//1.2.3").is_err());
        assert!(ProtocolVersion::from_str("/tari/igor/1.2.3/extra").is_err());
        assert!(ProtocolVersion::from_str("/tari/igor/1.2.3.4").is_err());
        assert!(Version::from_str("1.2.3.4").is_err());

        let version = ProtocolVersion::from_str("/tari/igor/1.2.3").unwrap();
        assert!(!version.is_compatible("/tari/igor/1.2.3/extra"));
        assert_ne!(version, "/tari/igor/1.2.3/extra".to_string());
    }

    #[test]
    fn semantic_version_eq_checks_all_leading_zero_components() {
        let v_0_0_1 = Version::new(0, 0, 1);
        assert!(v_0_0_1.semantic_version_eq(&Version::new(0, 0, 1)));
        assert!(!v_0_0_1.semantic_version_eq(&Version::new(0, 0, 2)));
        assert!(!v_0_0_1.semantic_version_eq(&Version::new(0, 1, 1)));
        assert!(!v_0_0_1.semantic_version_eq(&Version::new(1, 0, 1)));

        let v_0_1_0 = Version::new(0, 1, 0);
        assert!(v_0_1_0.semantic_version_eq(&Version::new(0, 1, 5)));
        assert!(!v_0_1_0.semantic_version_eq(&Version::new(0, 2, 0)));
        assert!(!v_0_1_0.semantic_version_eq(&Version::new(1, 1, 0)));

        let v_1_0_0 = Version::new(1, 0, 0);
        assert!(v_1_0_0.semantic_version_eq(&Version::new(1, 9, 9)));
        assert!(!v_1_0_0.semantic_version_eq(&Version::new(0, 0, 0)));
        assert!(!v_1_0_0.semantic_version_eq(&Version::new(2, 0, 0)));
    }
}
