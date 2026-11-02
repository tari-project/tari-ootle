//  Copyright 2022. The Tari Project
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

use minicbor::{CborLen, Decode, Encode};
use tari_bor::BorTag;
use tari_template_abi::{
    EngineOp,
    call_engine,
    rust::{collections::BTreeSet, fmt, prelude::*},
};
use tari_template_lib_types::{BinaryTag, NonFungibleId, ResourceAddress};

use crate::{
    args::{InvokeResult, ProofAction, ProofInvokeArg, ProofRef},
    types::{Amount, ResourceType},
};

const TAG: u64 = BinaryTag::ProofId.as_u64();

/// The unique identification of a proof during a transaction execution
#[derive(Debug, Clone, Copy, Encode, Decode, CborLen, PartialEq, Eq, Ord, PartialOrd, Hash)]
#[cbor(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ProofId(BorTag<u32, TAG>);

impl From<u32> for ProofId {
    fn from(value: u32) -> Self {
        Self(BorTag::new(value))
    }
}

impl fmt::Display for ProofId {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "ProofId({})", self.0.inner())
    }
}

/// Allows a user to prove ownership of a resource. Proofs only live during the execution of a transaction.
/// The main use case is to prove that the user has a specific badge during cross-template calls
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cbor(transparent)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize), serde(transparent))]
pub struct Proof {
    #[n(0)]
    id: ProofId,
}

impl Proof {
    pub const fn from_id(id: ProofId) -> Self {
        Self { id }
    }

    pub fn id(&self) -> ProofId {
        self.id
    }

    pub fn resource_address(&self) -> ResourceAddress {
        let resp: InvokeResult = call_engine(EngineOp::ProofInvoke, &ProofInvokeArg {
            proof_ref: ProofRef::Ref(self.id),
            action: ProofAction::GetResourceAddress,
            args: invoke_args![],
        });

        resp.decode()
            .expect("Proof GetResourceAddress returned invalid resource address")
    }

    pub fn resource_type(&self) -> ResourceType {
        let resp: InvokeResult = call_engine(EngineOp::ProofInvoke, &ProofInvokeArg {
            proof_ref: ProofRef::Ref(self.id),
            action: ProofAction::GetResourceType,
            args: invoke_args![],
        });

        resp.decode()
            .expect("Proof GetResourceType returned invalid resource type")
    }

    pub fn get_non_fungibles(&self) -> BTreeSet<NonFungibleId> {
        let resp: InvokeResult = call_engine(EngineOp::ProofInvoke, &ProofInvokeArg {
            proof_ref: ProofRef::Ref(self.id),
            action: ProofAction::GetNonFungibles,
            args: invoke_args![],
        });

        resp.decode()
            .expect("Proof GetNonFungibles returned invalid non-fungibles")
    }

    pub fn amount(&self) -> Amount {
        let resp: InvokeResult = call_engine(EngineOp::ProofInvoke, &ProofInvokeArg {
            proof_ref: ProofRef::Ref(self.id),
            action: ProofAction::GetAmount,
            args: invoke_args![],
        });

        resp.decode().expect("Proof GetAmount returned invalid amount")
    }

    /// Opens an authorization window over this proof for as long as the returned guard lives. Inside the window the
    /// proof satisfies this frame's own rules and is lent to every call the frame makes — one hop: the callee can
    /// use it but not pass it further. Prefer [`Self::authorize_with`], which bounds the window to one closure; a
    /// guard held across several calls lends to all of them.
    ///
    /// Only a proof this frame holds can be authorized: one it created, or one a callee returned. A proof received
    /// as an argument is shown, not held, and this panics for it.
    #[must_use = "ProofAccess must used"]
    pub fn authorize(&self) -> ProofAccess {
        self.try_authorize().expect("Proof authorization failed")
    }

    /// Runs `f` inside an authorization window over this proof. See [`Self::authorize`].
    pub fn authorize_with<F: FnOnce() -> R, R>(&self, f: F) -> R {
        let _auth = self.try_authorize().expect("Proof authorization failed");
        f()
    }

    /// Tries to open an authorization window over this proof. Fails when this frame does not hold the proof — it was
    /// shown as an argument, or it belongs to another frame — or when a window over it is already open.
    pub fn try_authorize(&self) -> Result<ProofAccess, NotAuthorized> {
        let resp: InvokeResult = call_engine(EngineOp::ProofInvoke, &ProofInvokeArg {
            proof_ref: ProofRef::Ref(self.id),
            action: ProofAction::Authorize,
            args: invoke_args![],
        });

        resp.decode::<Result<(), NotAuthorized>>()
            .expect("Proof Access error")?;
        Ok(ProofAccess { id: self.id })
    }

    /// Drop/destroy this proof. Only the frame holding the proof may drop it.
    pub fn drop(self) {
        drop_proof(self.id)
    }

    pub fn assert_resource(&self, resource_address: ResourceAddress) {
        assert_eq!(
            self.resource_address(),
            resource_address,
            "Proof of resource did not match {resource_address}"
        );
    }
}

/// Returned when a proof cannot be authorized
#[derive(Debug, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct NotAuthorized;

fn drop_proof(id: ProofId) {
    let resp: InvokeResult = call_engine(EngineOp::ProofInvoke, &ProofInvokeArg {
        proof_ref: ProofRef::Ref(id),
        action: ProofAction::Drop,
        args: invoke_args![],
    });

    resp.decode().expect("Proof drop error")
}

/// An open authorization window over a proof. The window closes when this is dropped; the proof itself stays held.
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct ProofAccess {
    #[n(0)]
    pub id: ProofId,
}

impl Drop for ProofAccess {
    fn drop(&mut self) {
        let resp: InvokeResult = call_engine(EngineOp::ProofInvoke, &ProofInvokeArg {
            proof_ref: ProofRef::Ref(self.id),
            action: ProofAction::DropAuthorize,
            args: invoke_args![],
        });

        resp.decode::<()>()
            .unwrap_or_else(|_| panic!("Drop failed for proof {}", self.id));
    }
}

/// A proof together with an open authorization window over it, created by [`Vault::authorize`]. Dropping it closes
/// the window and then destroys the proof, releasing the funds it locked.
///
/// [`Vault::authorize`]: crate::models::Vault::authorize
#[must_use = "the window closes and the proof is dropped as soon as ProofAuth is dropped"]
#[derive(Debug)]
pub struct ProofAuth {
    proof: Proof,
    access: Option<ProofAccess>,
}

impl ProofAuth {
    /// Opens a window over `proof`, which this frame must hold.
    pub fn new(proof: Proof) -> Self {
        let access = proof.authorize();
        Self {
            proof,
            access: Some(access),
        }
    }

    pub fn proof(&self) -> &Proof {
        &self.proof
    }
}

impl Drop for ProofAuth {
    fn drop(&mut self) {
        self.access.take();
        drop_proof(self.proof.id());
    }
}
