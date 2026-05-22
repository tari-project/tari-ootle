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
                other => {
                    return Err(Error::new(
                        flag.span(),
                        format!("unknown `#[template]` option `{other}`. Supported options: `skip_cbor_derives`"),
                    ));
                },
            }
        }

        Ok(options)
    }
}
