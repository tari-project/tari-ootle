//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-clause

//! The rule grammar of `set_access_rules!` and `set_owner_rule!`. Rules are written as in the template library's
//! `rule!` macro, e.g. `any_of(resource(admin_badge), public_key(pk))`. An address or key in a rule is a variable or
//! a string literal, and a string literal is read the same way as a variable passed to the manifest.

use std::{collections::BTreeSet, fmt, str::FromStr};

use proc_macro2::Ident;
use syn::{LitInt, LitStr, Token, ext::IdentExt, parenthesized, parse::ParseStream, punctuated::Punctuated, token};
use tari_engine_types::substate::SubstateId;
use tari_template_lib_types::{
    OwnerRule,
    access_rules::{AccessRule, ComponentAccessRules, RequireRule, RestrictedAccessRule, RuleRequirement},
    constants::{CALLER_COMPONENT_RESOURCE_ADDRESS, DIRECT_CALLER_TEMPLATE_RESOURCE_ADDRESS, TARI_TOKEN},
    crypto::RistrettoPublicKeyBytes,
};

use crate::{ManifestValue, error::ManifestError};

/// Looks up the value of a variable named in a rule.
pub(crate) trait RuleVariables {
    fn rule_variable(&self, name: &str) -> Result<&ManifestValue, ManifestError>;
}

/// The access rules of `set_access_rules!`.
#[derive(Debug, Clone)]
pub enum AccessRulesExpr {
    /// A variable holding encoded `ComponentAccessRules`.
    Var(Ident),
    /// `default = rule, method = rule, ..`. The default applies to every method without its own rule and is
    /// `deny_all` when omitted.
    Inline {
        default: Option<AccessRuleExpr>,
        methods: Vec<(String, AccessRuleExpr)>,
    },
}

/// The owner rule of `set_owner_rule!`.
#[derive(Debug, Clone)]
pub enum OwnerRuleExpr {
    /// A variable holding an encoded `OwnerRule`.
    Var(Ident),
    /// `owned_by_signer`: the transaction's signer becomes the owner.
    OwnedBySigner,
    /// `no_owner`.
    NoOwner,
    /// `public_key(pk)`.
    PublicKey(RuleArg),
    /// `access_rule(rule)`: anyone who satisfies the rule is the owner.
    AccessRule(AccessRuleExpr),
}

#[derive(Debug, Clone)]
pub enum AccessRuleExpr {
    AllowAll,
    DenyAll,
    Restricted(RestrictedRuleExpr),
}

#[derive(Debug, Clone)]
pub enum RestrictedRuleExpr {
    AnyOf(Vec<RestrictedRuleExpr>),
    AllOf(Vec<RestrictedRuleExpr>),
    MOfN {
        threshold: u16,
        requirements: Vec<RequirementExpr>,
    },
    Require(RequirementExpr),
}

#[derive(Debug, Clone)]
pub enum RequirementExpr {
    Resource(RuleArg),
    NonFungible(RuleArg),
    PublicKey(RuleArg),
    Component(RuleArg),
    Template(RuleArg),
    CallerComponent(RuleArg),
    DirectCallerTemplate(RuleArg),
    AnyCallerComponent,
    AnyCallerTemplate,
}

/// An address or key in a rule.
#[derive(Debug, Clone)]
pub enum RuleArg {
    Var(Ident),
    Value(ManifestValue),
}

impl fmt::Display for RuleArg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Var(ident) => write!(f, "'{ident}'"),
            Self::Value(value) => write!(f, "{value:?}"),
        }
    }
}

const REQUIREMENTS: &str = "resource, non_fungible, public_key, component, template, caller_component, \
                            direct_caller_template, any_caller_component, any_caller_template";

/// `rules_var` or `default = rule, method = rule, ..`.
pub(crate) fn parse_access_rules(input: ParseStream) -> syn::Result<AccessRulesExpr> {
    if input.peek(syn::Ident) && !input.peek2(Token![=]) {
        let var: Ident = input.parse()?;
        input.parse::<Option<Token![,]>>()?;
        return Ok(AccessRulesExpr::Var(var));
    }

    let mut default = None;
    let mut methods = Vec::new();
    let mut seen = BTreeSet::new();
    let entries = Punctuated::<(Ident, AccessRuleExpr), Token![,]>::parse_terminated_with(input, |input| {
        // `Ident::parse_any` admits a method named after a keyword, e.g. `r#default`.
        let key = input.call(Ident::parse_any)?;
        input.parse::<Token![=]>()?;
        Ok((key, parse_access_rule(input)?))
    })?;
    if entries.is_empty() {
        return Err(input.error("expected access rules, e.g. `default = deny_all, method = rule`"));
    }
    for (key, rule) in entries {
        // `default` names the default rule; `r#default` names a method called `default`.
        if key == "default" {
            if default.replace(rule).is_some() {
                return Err(syn::Error::new_spanned(key, "duplicate `default` rule"));
            }
            continue;
        }
        let method = key.unraw().to_string();
        if !seen.insert(method.clone()) {
            return Err(syn::Error::new_spanned(
                key,
                format!("duplicate rule for method `{method}`"),
            ));
        }
        methods.push((method, rule));
    }
    Ok(AccessRulesExpr::Inline { default, methods })
}

/// `owned_by_signer`, `no_owner`, `public_key(pk)`, `access_rule(rule)` or a variable.
pub(crate) fn parse_owner_rule(input: ParseStream) -> syn::Result<OwnerRuleExpr> {
    let name: Ident = input.parse()?;
    let rule = match name.to_string().as_str() {
        "owned_by_signer" => OwnerRuleExpr::OwnedBySigner,
        "no_owner" => OwnerRuleExpr::NoOwner,
        "public_key" => OwnerRuleExpr::PublicKey(parse_single_arg(input, &name)?),
        "access_rule" => {
            let content;
            parenthesized!(content in input);
            let rule = parse_access_rule(&content)?;
            expect_end(&content)?;
            OwnerRuleExpr::AccessRule(rule)
        },
        _ if input.peek(token::Paren) => {
            return Err(syn::Error::new_spanned(
                &name,
                format!(
                    "unknown owner rule `{name}`, expected owned_by_signer, no_owner, public_key(pk) or \
                     access_rule(rule)"
                ),
            ));
        },
        _ => OwnerRuleExpr::Var(name),
    };
    input.parse::<Option<Token![,]>>()?;
    Ok(rule)
}

/// `allow_all`, `deny_all` or a restricted rule.
pub(crate) fn parse_access_rule(input: ParseStream) -> syn::Result<AccessRuleExpr> {
    let name: Ident = input.parse()?;
    match name.to_string().as_str() {
        "allow_all" => Ok(AccessRuleExpr::AllowAll),
        "deny_all" => Ok(AccessRuleExpr::DenyAll),
        _ => Ok(AccessRuleExpr::Restricted(parse_restricted_named(&name, input)?)),
    }
}

fn parse_restricted(input: ParseStream) -> syn::Result<RestrictedRuleExpr> {
    let name: Ident = input.parse()?;
    parse_restricted_named(&name, input)
}

fn parse_restricted_named(name: &Ident, input: ParseStream) -> syn::Result<RestrictedRuleExpr> {
    match name.to_string().as_str() {
        "any_of" | "all_of" => {
            let content;
            let paren = parenthesized!(content in input);
            let rules = Punctuated::<_, Token![,]>::parse_terminated_with(&content, parse_restricted)?;
            if rules.is_empty() {
                return Err(syn::Error::new(
                    paren.span.join(),
                    format!("{name}(..) needs at least one rule"),
                ));
            }
            let rules = rules.into_iter().collect();
            Ok(if name == "any_of" {
                RestrictedRuleExpr::AnyOf(rules)
            } else {
                RestrictedRuleExpr::AllOf(rules)
            })
        },
        "m_of_n" => {
            let content;
            let paren = parenthesized!(content in input);
            let threshold: LitInt = content.parse()?;
            let threshold = threshold.base10_parse::<u16>()?;
            content.parse::<Token![,]>()?;
            let requirements = Punctuated::<_, Token![,]>::parse_terminated_with(&content, parse_requirement)?;
            if requirements.is_empty() {
                return Err(syn::Error::new(
                    paren.span.join(),
                    "m_of_n(..) needs at least one requirement",
                ));
            }
            Ok(RestrictedRuleExpr::MOfN {
                threshold,
                requirements: requirements.into_iter().collect(),
            })
        },
        "allow_all" | "deny_all" => Err(syn::Error::new_spanned(
            name,
            format!("`{name}` is a whole rule and cannot be combined with other rules"),
        )),
        _ => Ok(RestrictedRuleExpr::Require(parse_requirement_named(name, input)?)),
    }
}

fn parse_requirement(input: ParseStream) -> syn::Result<RequirementExpr> {
    let name: Ident = input.parse()?;
    parse_requirement_named(&name, input)
}

fn parse_requirement_named(name: &Ident, input: ParseStream) -> syn::Result<RequirementExpr> {
    let requirement = match name.to_string().as_str() {
        "resource" => RequirementExpr::Resource(parse_single_arg(input, name)?),
        "non_fungible" => RequirementExpr::NonFungible(parse_single_arg(input, name)?),
        "public_key" => RequirementExpr::PublicKey(parse_single_arg(input, name)?),
        "component" => RequirementExpr::Component(parse_single_arg(input, name)?),
        "template" => RequirementExpr::Template(parse_single_arg(input, name)?),
        "caller_component" => RequirementExpr::CallerComponent(parse_single_arg(input, name)?),
        "direct_caller_template" => RequirementExpr::DirectCallerTemplate(parse_single_arg(input, name)?),
        "any_caller_component" => RequirementExpr::AnyCallerComponent,
        "any_caller_template" => RequirementExpr::AnyCallerTemplate,
        _ => {
            return Err(syn::Error::new_spanned(
                name,
                format!("unknown rule `{name}`, expected allow_all, deny_all, any_of, all_of, m_of_n, {REQUIREMENTS}"),
            ));
        },
    };
    if matches!(
        requirement,
        RequirementExpr::AnyCallerComponent | RequirementExpr::AnyCallerTemplate
    ) && input.peek(token::Paren)
    {
        return Err(syn::Error::new_spanned(name, format!("`{name}` takes no arguments")));
    }
    Ok(requirement)
}

/// The single argument in `name(arg)`: a variable, `TARI` or a string literal.
fn parse_single_arg(input: ParseStream, name: &Ident) -> syn::Result<RuleArg> {
    if !input.peek(token::Paren) {
        return Err(syn::Error::new_spanned(
            name,
            format!("`{name}` takes one argument: `{name}(..)`"),
        ));
    }
    let content;
    parenthesized!(content in input);
    let arg = if content.peek(LitStr) {
        let lit: LitStr = content.parse()?;
        let value = ManifestValue::from_str(&lit.value()).map_err(|e| syn::Error::new_spanned(&lit, e))?;
        RuleArg::Value(value)
    } else {
        let ident: Ident = content.parse()?;
        if ident == "TARI" {
            RuleArg::Value(ManifestValue::SubstateId(TARI_TOKEN.into()))
        } else {
            RuleArg::Var(ident)
        }
    };
    expect_end(&content)?;
    Ok(arg)
}

fn expect_end(input: ParseStream) -> syn::Result<()> {
    if input.is_empty() {
        Ok(())
    } else {
        Err(input.error("unexpected tokens"))
    }
}

impl AccessRulesExpr {
    pub(crate) fn build(&self, vars: &impl RuleVariables) -> Result<ComponentAccessRules, ManifestError> {
        let (default, methods) = match self {
            Self::Var(ident) => return decode_var(vars, ident),
            Self::Inline { default, methods } => (default, methods),
        };
        let mut rules = ComponentAccessRules::new();
        if let Some(default) = default {
            rules = rules.default(default.build(vars)?);
        }
        for (method, rule) in methods {
            rules = rules.add_method_rule(method.as_str(), rule.build(vars)?);
        }
        Ok(rules)
    }
}

impl OwnerRuleExpr {
    pub(crate) fn build(&self, vars: &impl RuleVariables) -> Result<OwnerRule, ManifestError> {
        Ok(match self {
            Self::Var(ident) => return decode_var(vars, ident),
            Self::OwnedBySigner => OwnerRule::OwnedBySigner,
            Self::NoOwner => OwnerRule::None,
            Self::PublicKey(arg) => OwnerRule::ByPublicKey(public_key(vars, arg)?),
            Self::AccessRule(rule) => OwnerRule::ByAccessRule(rule.build(vars)?),
        })
    }
}

impl AccessRuleExpr {
    fn build(&self, vars: &impl RuleVariables) -> Result<AccessRule, ManifestError> {
        let rule = match self {
            Self::AllowAll => AccessRule::AllowAll,
            Self::DenyAll => AccessRule::DenyAll,
            Self::Restricted(rule) => AccessRule::Restricted(rule.build(vars)?),
        };
        if let Some(invalid) = rule.find_invalid_rule() {
            return Err(ManifestError::InvalidInstruction {
                reason: format!("invalid access rule: {invalid:?}"),
            });
        }
        Ok(rule)
    }
}

impl RestrictedRuleExpr {
    fn build(&self, vars: &impl RuleVariables) -> Result<RestrictedAccessRule, ManifestError> {
        Ok(match self {
            Self::AnyOf(rules) => RestrictedAccessRule::AnyOf(build_all(rules, |r| r.build(vars))?),
            Self::AllOf(rules) => RestrictedAccessRule::AllOf(build_all(rules, |r| r.build(vars))?),
            Self::MOfN {
                threshold,
                requirements,
            } => RestrictedAccessRule::Require(RequireRule::MOfN(
                *threshold,
                build_all(requirements, |r| r.build(vars))?,
            )),
            Self::Require(requirement) => RestrictedAccessRule::Require(RequireRule::Require(requirement.build(vars)?)),
        })
    }
}

impl RequirementExpr {
    fn build(&self, vars: &impl RuleVariables) -> Result<RuleRequirement, ManifestError> {
        Ok(match self {
            Self::Resource(arg) => match substate_id(vars, arg, "resource")? {
                SubstateId::Resource(address) => RuleRequirement::Resource(address),
                other => return Err(wrong_address(arg, "resource", &other)),
            },
            Self::NonFungible(arg) => match substate_id(vars, arg, "non_fungible")? {
                SubstateId::NonFungible(address) => RuleRequirement::NonFungibleAddress(address),
                other => return Err(wrong_address(arg, "non_fungible", &other)),
            },
            Self::PublicKey(arg) => RuleRequirement::from(public_key(vars, arg)?),
            Self::Component(arg) => match substate_id(vars, arg, "component")? {
                SubstateId::Component(address) => RuleRequirement::ScopedToComponent(address),
                other => return Err(wrong_address(arg, "component", &other)),
            },
            Self::Template(arg) => match substate_id(vars, arg, "template")? {
                SubstateId::Template(address) => RuleRequirement::ScopedToTemplate(address.as_template_address()),
                other => return Err(wrong_address(arg, "template", &other)),
            },
            Self::CallerComponent(arg) => match substate_id(vars, arg, "caller_component")? {
                SubstateId::Component(address) => RuleRequirement::CallerComponent(address),
                other => return Err(wrong_address(arg, "caller_component", &other)),
            },
            Self::DirectCallerTemplate(arg) => match substate_id(vars, arg, "direct_caller_template")? {
                SubstateId::Template(address) => RuleRequirement::DirectCallerTemplate(address.as_template_address()),
                other => return Err(wrong_address(arg, "direct_caller_template", &other)),
            },
            Self::AnyCallerComponent => RuleRequirement::Resource(CALLER_COMPONENT_RESOURCE_ADDRESS),
            Self::AnyCallerTemplate => RuleRequirement::Resource(DIRECT_CALLER_TEMPLATE_RESOURCE_ADDRESS),
        })
    }
}

fn build_all<T, U>(items: &[T], build: impl Fn(&T) -> Result<U, ManifestError>) -> Result<Box<[U]>, ManifestError> {
    items.iter().map(build).collect()
}

fn arg_value<'a>(vars: &'a impl RuleVariables, arg: &'a RuleArg) -> Result<&'a ManifestValue, ManifestError> {
    match arg {
        RuleArg::Var(ident) => vars.rule_variable(&ident.to_string()),
        RuleArg::Value(value) => Ok(value),
    }
}

fn substate_id(vars: &impl RuleVariables, arg: &RuleArg, rule: &str) -> Result<SubstateId, ManifestError> {
    arg_value(vars, arg)?.as_address().cloned().ok_or_else(|| {
        ManifestError::InvalidVariableType(format!("{rule}(..) needs an address, but {arg} is not an address"))
    })
}

fn wrong_address(arg: &RuleArg, rule: &str, id: &SubstateId) -> ManifestError {
    ManifestError::InvalidVariableType(format!(
        "{rule}(..) needs a {} address, but {arg} is {id}",
        match rule {
            "resource" => "resource",
            "non_fungible" => "non-fungible",
            "component" | "caller_component" => "component",
            _ => "template",
        }
    ))
}

fn public_key(vars: &impl RuleVariables, arg: &RuleArg) -> Result<RistrettoPublicKeyBytes, ManifestError> {
    match arg_value(vars, arg)? {
        ManifestValue::Value(tari_bor::Value::Bytes(bytes)) => RistrettoPublicKeyBytes::from_bytes(bytes)
            .map_err(|e| ManifestError::InvalidVariableType(format!("public_key({arg}) is not a public key: {e}"))),
        _ => Err(ManifestError::InvalidVariableType(format!(
            "public_key(..) needs hex public key bytes, but {arg} is not bytes"
        ))),
    }
}

fn decode_var<T>(vars: &impl RuleVariables, ident: &Ident) -> Result<T, ManifestError>
where T: for<'b> tari_bor::Decode<'b, ()> {
    match vars.rule_variable(&ident.to_string())? {
        ManifestValue::Value(value) => tari_bor::from_value(value)
            .map_err(|e| ManifestError::InvalidVariableType(format!("'{ident}' does not hold the expected rule: {e}"))),
        value => Err(ManifestError::InvalidVariableType(format!(
            "'{ident}' must hold an encoded rule, but holds {value:?}"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use syn::parse::Parser;

    use super::*;

    struct NoVars;

    impl RuleVariables for NoVars {
        fn rule_variable(&self, name: &str) -> Result<&ManifestValue, ManifestError> {
            Err(ManifestError::UndefinedVariable { name: name.to_string() })
        }
    }

    fn access_rule(src: &str) -> Result<AccessRule, String> {
        let expr = parse_access_rule.parse_str(src).map_err(|e| e.to_string())?;
        expr.build(&NoVars).map_err(|e| e.to_string())
    }

    #[test]
    fn whole_rules_cannot_be_combined() {
        let err = access_rule("any_of(allow_all, resource(TARI))").unwrap_err();
        assert!(err.contains("whole rule"), "{err}");
    }

    #[test]
    fn m_of_n_threshold_must_fit_its_requirements() {
        let err = access_rule("m_of_n(2, resource(TARI))").unwrap_err();
        assert!(err.contains("invalid access rule"), "{err}");
    }

    #[test]
    fn any_caller_requirements_take_no_arguments() {
        let err = access_rule("any_caller_component(TARI)").unwrap_err();
        assert!(err.contains("takes no arguments"), "{err}");
    }

    #[test]
    fn a_key_rule_needs_a_key() {
        let err = access_rule("public_key(TARI)").unwrap_err();
        assert!(err.contains("not bytes"), "{err}");
    }

    #[test]
    fn a_resource_rule_needs_a_resource_address() {
        let pk = "0".repeat(64);
        let err = access_rule(&format!(r#"resource("{pk}")"#)).unwrap_err();
        assert!(err.contains("not an address"), "{err}");
    }

    #[test]
    fn a_method_called_default_is_written_raw() {
        let rules = parse_access_rules
            .parse_str("default = allow_all, r#default = deny_all")
            .unwrap()
            .build(&NoVars)
            .unwrap();
        assert_eq!(*rules.get_method_access_rule("default"), AccessRule::DenyAll);
        assert_eq!(*rules.get_method_access_rule("other"), AccessRule::AllowAll);
    }

    #[test]
    fn a_method_rule_may_be_given_only_once() {
        let err = parse_access_rules
            .parse_str("mint = allow_all, mint = deny_all")
            .unwrap_err()
            .to_string();
        assert!(err.contains("duplicate rule for method `mint`"), "{err}");
    }
}
