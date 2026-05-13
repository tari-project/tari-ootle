//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! Extract rustdoc comments from public functions of a template module.
//!
//! Parses a Rust source file with `syn`, locates the `#[template]` `mod` block,
//! finds its public template type, and walks `impl` blocks for that type
//! collecting `pub fn` items together with any `///` / `#[doc = "..."]` attributes.

use std::{fs, path::Path};

use tari_ootle_template_metadata::FunctionMetadata;

/// Read the given source file and return one [`FunctionMetadata`] per public template function.
///
/// Returns an empty vec if the file contains no `#[template]` mod or no eligible public functions.
pub fn extract_function_docs(source: &Path) -> Result<Vec<FunctionMetadata>, DocExtractError> {
    let content = fs::read_to_string(source).map_err(|e| DocExtractError::Io {
        path: source.to_path_buf(),
        source: e,
    })?;
    let file = syn::parse_file(&content).map_err(|e| DocExtractError::Parse {
        path: source.to_path_buf(),
        source: e,
    })?;
    Ok(extract_from_file(&file))
}

fn extract_from_file(file: &syn::File) -> Vec<FunctionMetadata> {
    for item in &file.items {
        if let syn::Item::Mod(m) = item &&
            has_template_attr(&m.attrs)
        {
            return extract_from_mod(m);
        }
    }
    Vec::new()
}

fn has_template_attr(attrs: &[syn::Attribute]) -> bool {
    attrs
        .iter()
        .any(|attr| attr.path().segments.last().is_some_and(|seg| seg.ident == "template"))
}

fn extract_from_mod(m: &syn::ItemMod) -> Vec<FunctionMetadata> {
    let Some((_, items)) = &m.content else {
        return Vec::new();
    };

    // The template name is taken from the first public struct or enum (mirrors template_macros).
    let template_name = items.iter().find_map(|item| match item {
        syn::Item::Struct(s) if matches!(s.vis, syn::Visibility::Public(_)) => Some(s.ident.clone()),
        syn::Item::Enum(e) if matches!(e.vis, syn::Visibility::Public(_)) => Some(e.ident.clone()),
        _ => None,
    });

    let Some(template_name) = template_name else {
        return Vec::new();
    };

    let mut out = Vec::new();
    for item in items {
        let syn::Item::Impl(impl_block) = item else {
            continue;
        };
        if impl_block.trait_.is_some() {
            continue;
        }
        let syn::Type::Path(self_ty) = &*impl_block.self_ty else {
            continue;
        };
        if !self_ty.path.is_ident(&template_name) {
            continue;
        }

        for impl_item in &impl_block.items {
            let syn::ImplItem::Fn(f) = impl_item else { continue };
            if !matches!(f.vis, syn::Visibility::Public(_)) {
                continue;
            }
            out.push(FunctionMetadata {
                name: f.sig.ident.to_string(),
                doc: extract_doc_lines(&f.attrs),
            });
        }
    }
    out
}

fn extract_doc_lines(attrs: &[syn::Attribute]) -> String {
    let mut lines: Vec<String> = Vec::new();
    for attr in attrs {
        if !attr.path().is_ident("doc") {
            continue;
        }
        let syn::Meta::NameValue(nv) = &attr.meta else {
            continue;
        };
        let syn::Expr::Lit(syn::ExprLit {
            lit: syn::Lit::Str(s), ..
        }) = &nv.value
        else {
            continue;
        };
        let raw = s.value();
        // Rustdoc strips one leading space if present, matching `///  text` -> ` text`.
        let trimmed = raw.strip_prefix(' ').unwrap_or(&raw);
        lines.push(trimmed.to_string());
    }
    lines.join("\n")
}

#[derive(Debug, thiserror::Error)]
pub enum DocExtractError {
    #[error("Failed to read template source '{path}': {source}")]
    Io {
        path: std::path::PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("Failed to parse template source '{path}': {source}")]
    Parse {
        path: std::path::PathBuf,
        #[source]
        source: syn::Error,
    },
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(src: &str) -> Vec<FunctionMetadata> {
        let file = syn::parse_file(src).unwrap();
        extract_from_file(&file)
    }

    #[test]
    fn extracts_single_function_doc() {
        let src = r#"
            #[template]
            mod tpl {
                pub struct Tpl;
                impl Tpl {
                    /// Mint a new token.
                    pub fn mint() {}
                }
            }
        "#;
        let out = parse(src);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "mint");
        assert_eq!(out[0].doc, "Mint a new token.");
    }

    #[test]
    fn joins_multi_line_doc() {
        let src = r#"
            #[template]
            mod tpl {
                pub struct Tpl;
                impl Tpl {
                    /// First line.
                    /// Second line.
                    pub fn foo() {}
                }
            }
        "#;
        let out = parse(src);
        assert_eq!(out[0].doc, "First line.\nSecond line.");
    }

    #[test]
    fn skips_non_public_functions() {
        let src = r#"
            #[template]
            mod tpl {
                pub struct Tpl;
                impl Tpl {
                    /// public
                    pub fn pub_fn() {}
                    /// private
                    fn priv_fn() {}
                }
            }
        "#;
        let out = parse(src);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "pub_fn");
    }

    #[test]
    fn includes_function_with_no_doc() {
        let src = r#"
            #[template]
            mod tpl {
                pub struct Tpl;
                impl Tpl {
                    pub fn no_docs() {}
                }
            }
        "#;
        let out = parse(src);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "no_docs");
        assert_eq!(out[0].doc, "");
    }

    #[test]
    fn ignores_impls_of_other_types() {
        let src = r#"
            #[template]
            mod tpl {
                pub struct Tpl;
                pub struct Helper;
                impl Helper {
                    /// not a template fn
                    pub fn other() {}
                }
                impl Tpl {
                    /// is a template fn
                    pub fn here() {}
                }
            }
        "#;
        let out = parse(src);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "here");
    }

    #[test]
    fn ignores_trait_impls() {
        let src = r#"
            #[template]
            mod tpl {
                pub struct Tpl;
                impl Default for Tpl {
                    fn default() -> Self { Tpl }
                }
                impl Tpl {
                    /// real fn
                    pub fn real() {}
                }
            }
        "#;
        let out = parse(src);
        assert_eq!(out.len(), 1);
        assert_eq!(out[0].name, "real");
    }

    #[test]
    fn returns_empty_when_no_template_mod() {
        let src = r#"
            pub struct NotATemplate;
            impl NotATemplate {
                pub fn foo() {}
            }
        "#;
        let out = parse(src);
        assert!(out.is_empty());
    }

    #[test]
    fn preserves_source_order() {
        let src = r#"
            #[template]
            mod tpl {
                pub struct Tpl;
                impl Tpl {
                    pub fn a() {}
                    pub fn b() {}
                    pub fn c() {}
                }
            }
        "#;
        let out = parse(src);
        let names: Vec<_> = out.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["a", "b", "c"]);
    }
}
