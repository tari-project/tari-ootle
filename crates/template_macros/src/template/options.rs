//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Parsing for the comma-separated flag list inside `#[template(...)]`.
//!
//! Currently understood flags:
//!
//! - `skip_cbor_derives` — suppress the default `#[derive(minicbor::Encode, Decode, CborLen)]` and field/variant tag
//!   injection. The template author becomes responsible for providing their own derives and `#[n(N)]` / `#[b(N)]` /
//!   `#[cbor(n(N))]` numbering. Useful when the wire format needs to differ from the macro's default ordering (e.g.
//!   preserving an existing on-disk format across a field reorder).
//! - `stateless` — declare a component-less template whose public API is a set of free `pub fn` items rather than a
//!   component with methods. No struct is interpreted as the component, the template name is taken from the module
//!   identifier, and `&self`/`&mut self` methods, `-> Self` constructors and `#[migration]` functions are rejected.
//!   Composes with `skip_cbor_derives` (e.g. `#[template(stateless, skip_cbor_derives)]`).

use syn::{
    Error,
    Ident,
    Result,
    Token,
    parse::{Parse, ParseStream},
    punctuated::Punctuated,
};

/// Options parsed from the attribute arguments of `#[template(...)]`.
#[derive(Debug, Default, Clone, Copy)]
pub struct TemplateOptions {
    /// When `true`, the macro will not inject `#[derive(minicbor::Encode, Decode, CborLen)]`
    /// onto template structs/enums and will not auto-assign `#[n(N)]` tags to their fields or
    /// variants. The author retains full control over the CBOR wire format.
    pub skip_cbor_derives: bool,
    /// When `true`, the module describes a component-less, stateless template. Its public API is
    /// the set of free `pub fn` items in the module (no struct is treated as a component) and the
    /// template name is the module identifier. Methods (`&self`/`&mut self`), `-> Self`
    /// constructors and `#[migration]` functions are rejected during parsing.
    pub stateless: bool,
}

impl Parse for TemplateOptions {
    fn parse(input: ParseStream) -> Result<Self> {
        let mut options = Self::default();
        if input.is_empty() {
            return Ok(options);
        }

        let flags = Punctuated::<Ident, Token![,]>::parse_terminated(input)?;
        for flag in flags {
            match flag.to_string().as_str() {
                "skip_cbor_derives" => options.skip_cbor_derives = true,
                "stateless" => options.stateless = true,
                other => {
                    return Err(Error::new(
                        flag.span(),
                        format!(
                            "unknown `#[template]` option `{other}`. Supported options: `skip_cbor_derives`, \
                             `stateless`"
                        ),
                    ));
                },
            }
        }

        Ok(options)
    }
}
