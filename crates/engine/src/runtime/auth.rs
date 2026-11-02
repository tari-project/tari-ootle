//   Copyright 2023 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::fmt::Display;

use indexmap::IndexSet;
use tari_template_lib::{
    models::ProofId,
    types::{ComponentAddress, NonFungibleAddress, ResourceAddress, TemplateAddress},
};

#[derive(Debug, Clone, Default)]
pub struct AuthParams {
    pub initial_ownership_proofs: IndexSet<NonFungibleAddress>,
}

#[derive(Debug, Clone)]
pub struct AuthorizationScope {
    /// Virtual proofs are system-issued non-fungibles that exist for no longer than the execution e.g. derived from
    /// the transaction signer public key
    virtual_proofs: IndexSet<NonFungibleAddress>,

    /// System-issued badges naming the frame's immediate caller: the caller's component (when it has one) and its
    /// template. Held per frame rather than in the transaction-wide `virtual_proofs` so that a callee only ever sees
    /// the frame directly below it — the badges are re-derived at every push and never merged into a parent or child
    /// scope. There is no path from a badge to a [`ProofId`], so a callee cannot forward the identity it was called
    /// with.
    caller_badges: IndexSet<NonFungibleAddress>,

    /// The proofs this frame has opened an authorization window over. Only a proof the frame holds can be here, so
    /// the frame may also pass, return or drop any of them.
    proofs: IndexSet<ProofId>,

    /// The proofs lent to this frame by its caller: the caller's `proofs` as they stood when this frame was pushed.
    /// They satisfy this frame's rules exactly as `proofs` do, but the frame does not hold them — it cannot open a
    /// window over them, pass them on, return or drop them — so the loan ends with this frame and never reaches
    /// its own callees.
    granted: IndexSet<ProofId>,
}

impl AuthorizationScope {
    pub fn new(virtual_proofs: IndexSet<NonFungibleAddress>) -> Self {
        Self {
            virtual_proofs,
            caller_badges: IndexSet::new(),
            proofs: IndexSet::new(),
            granted: IndexSet::new(),
        }
    }

    pub fn empty() -> Self {
        Self::new(IndexSet::new())
    }

    /// The scope a frame pushed by a top-level instruction starts with: the transaction's signer badges, and the
    /// proofs the transaction has authorized at that point on loan.
    pub(super) fn for_top_level_frame(&self) -> Self {
        Self {
            virtual_proofs: self.virtual_proofs.clone(),
            caller_badges: IndexSet::new(),
            proofs: IndexSet::new(),
            granted: self.proofs.clone(),
        }
    }

    /// Lends this frame's authorized proofs to the frame it is pushing. Signer badges are not carried: they stamp
    /// the first frame only.
    pub(super) fn lend_to_callee(&self, callee: &mut Self) {
        callee.granted = self.proofs.clone();
    }

    /// Stamps the identity of the frame that is pushing the frame this scope belongs to. The component badge is
    /// omitted for a static function frame, which has no component identity.
    pub(super) fn set_caller(&mut self, component: Option<ComponentAddress>, template: TemplateAddress) {
        self.caller_badges.clear();
        if let Some(component) = component {
            self.caller_badges
                .insert(NonFungibleAddress::caller_component_badge(component));
        }
        self.caller_badges
            .insert(NonFungibleAddress::direct_caller_template_badge(template));
    }

    fn badges(&self) -> impl Iterator<Item = &NonFungibleAddress> {
        self.virtual_proofs.iter().chain(&self.caller_badges)
    }

    pub fn contains_badge(&self, nf_address: &NonFungibleAddress) -> bool {
        self.virtual_proofs.contains(nf_address) || self.caller_badges.contains(nf_address)
    }

    pub fn contains_badge_of_resource(&self, resource_address: &ResourceAddress) -> bool {
        self.badges().any(|badge| badge.resource_address() == resource_address)
    }

    /// The scope a restricted top-level frame starts with: the signer badges and nothing on loan.
    pub(super) fn badges_only(&self) -> Self {
        Self::new(self.virtual_proofs.clone())
    }

    /// The proofs this frame has authorized itself.
    pub fn proofs(&self) -> &IndexSet<ProofId> {
        &self.proofs
    }

    /// Every proof that satisfies this frame's rules: its own windows and its caller's loan.
    pub fn authorizing_proofs(&self) -> impl Iterator<Item = &ProofId> {
        self.proofs.iter().chain(&self.granted)
    }

    pub fn add_proof(&mut self, proof_id: ProofId) {
        self.proofs.insert(proof_id);
    }

    pub fn remove_proof(&mut self, proof_id: &ProofId) -> bool {
        self.proofs.swap_remove(proof_id)
    }

    pub fn contains_proof(&self, proof_id: &ProofId) -> bool {
        self.proofs.contains(proof_id)
    }
}

impl Display for AuthorizationScope {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Virtual: [")?;
        for badge in self.badges() {
            write!(f, "{}", badge)?;
        }
        write!(f, "], Proofs: [")?;
        for proof in &self.proofs {
            write!(f, "{}", proof)?;
        }
        write!(f, "], Granted: [")?;
        for proof in &self.granted {
            write!(f, "{}", proof)?;
        }
        write!(f, "]")
    }
}
