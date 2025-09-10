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

//! The collection of all struct definitions that represent data in the Tari network (e.g., resources, components,
//! proofs, etc.)

mod account;
mod address_allocation;
pub mod address_prefixes;
mod binary_tag;
mod bucket;
mod claimed_output_tombstone;
mod component;
mod confidential_proof;
mod encrypted_data;
mod metadata;
mod non_fungible;
mod proof;
mod resource;
mod stealth;
mod system;
mod unspent_output;
mod vault;
mod viewable_balance;

pub use account::*;
pub use address_allocation::*;
pub use binary_tag::*;
pub use bucket::*;
pub use claimed_output_tombstone::*;
pub use component::*;
pub use confidential_proof::*;
pub use encrypted_data::*;
pub use metadata::*;
pub use non_fungible::*;
pub use proof::*;
pub use resource::ResourceAddress;
pub use stealth::*;
pub use system::*;
pub use unspent_output::*;
pub use vault::*;
pub use viewable_balance::*;
