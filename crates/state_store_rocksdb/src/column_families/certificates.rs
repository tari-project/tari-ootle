//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

use tari_consensus_types::{PcId, ProposalCertificate, TcId, TimeoutCertificate};
use tari_ootle_common_types::Epoch;

use crate::{
    codecs::{DefaultVersionedCodec, EpochCodec, FixedBytesCodec32},
    traits::{Cf, QueryCf},
    versioned_types::{VersionedProposalCertificate, VersionedTimeoutCertificate},
};

pub mod proposal {
    use super::*;

    pub struct ProposalCertificateCf;

    impl Cf for ProposalCertificateCf {
        type Key = (Epoch, PcId);
        type KeyCodec = (EpochCodec, FixedBytesCodec32);
        type Value = ProposalCertificate;
        type ValueCodec = DefaultVersionedCodec<VersionedProposalCertificate>;

        fn name() -> &'static str {
            "proposal_certificates"
        }
    }

    pub struct ByEpochQuery;

    impl QueryCf for ByEpochQuery {
        type Cf = ProposalCertificateCf;
        type Key = Epoch;
        type KeyCodec = EpochCodec;
    }
}

pub mod timeout {
    use super::*;

    pub struct TimeoutCertificateCf;

    impl Cf for TimeoutCertificateCf {
        type Key = (Epoch, TcId);
        type KeyCodec = (EpochCodec, FixedBytesCodec32);
        type Value = TimeoutCertificate;
        type ValueCodec = DefaultVersionedCodec<VersionedTimeoutCertificate>;

        fn name() -> &'static str {
            "timeout_certificates"
        }
    }

    pub struct ByEpochQuery;

    impl QueryCf for ByEpochQuery {
        type Cf = TimeoutCertificateCf;
        type Key = Epoch;
        type KeyCodec = EpochCodec;
    }
}
