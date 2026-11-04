//  Copyright 2022. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};

use crate::{
    rust::{boxed::Box, string::String, vec::Vec},
    version::WasmAbiVersion,
};

/// A template's definition, always in the current layout.
///
/// Decoding accepts every layout a published template can carry and converts it to the current one, so code reading a
/// definition sees a single shape. Encoding always writes the current layout.
#[derive(Debug, Clone, Encode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "serde", serde(from = "VersionedTemplateDef"))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum TemplateDef {
    #[n(1)]
    V2(#[n(0)] TemplateDefV2),
}

impl TemplateDef {
    pub fn template_name(&self) -> &str {
        match self {
            TemplateDef::V2(def) => def.template_name.as_str(),
        }
    }

    pub fn abi_version(&self) -> u16 {
        match self {
            TemplateDef::V2(def) => def.abi_version,
        }
    }

    pub fn get_function(&self, name: &str) -> Option<&FunctionDef> {
        match self {
            TemplateDef::V2(def) => def.get_function(name),
        }
    }

    pub fn functions(&self) -> &[FunctionDef] {
        match self {
            TemplateDef::V2(def) => &def.functions,
        }
    }

    /// Encodes the template definition with a length prefix, which is required for passing data to the engine in wasm.
    /// The length prefix is a 4-byte little-endian integer representing the total length of the encoded data (including
    /// the prefix itself).
    #[cfg(all(not(target_arch = "wasm32"), feature = "std"))]
    pub fn encode_for_wasm_embedding(&self) -> Result<Vec<u8>, tari_bor::BorError> {
        use std::io::Write;

        use tari_bor::{encode_into_writer, encoded_len};

        use crate::WASM_PTR_SIZE;
        let data_len = encoded_len(self);
        // for the length prefix
        let mut buf = Vec::with_capacity(data_len + WASM_PTR_SIZE);
        let full_len = data_len + WASM_PTR_SIZE;
        let writer = &mut buf;
        // length prefix
        writer.write_all(&(full_len as u32).to_le_bytes())?;
        encode_into_writer(self, writer)?;
        Ok(buf)
    }
}

impl<'b, C> Decode<'b, C> for TemplateDef {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        VersionedTemplateDef::decode(d, ctx).map(Self::from)
    }
}

/// Every template definition layout, as encoded. The variant indices match [`TemplateDef`]'s, so the current layout
/// decodes and encodes identically through either type.
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub enum VersionedTemplateDef {
    #[n(0)]
    V1(#[n(0)] TemplateDefV1),
    #[n(1)]
    V2(#[n(0)] TemplateDefV2),
}

impl From<VersionedTemplateDef> for TemplateDef {
    fn from(def: VersionedTemplateDef) -> Self {
        match def {
            VersionedTemplateDef::V1(def) => TemplateDef::V2(def.into()),
            VersionedTemplateDef::V2(def) => TemplateDef::V2(def),
        }
    }
}

#[derive(Debug, Clone, Default, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct TemplateDefV2 {
    #[n(0)]
    pub template_name: String,
    #[n(1)]
    pub abi_version: WasmAbiVersion,
    #[n(2)]
    pub functions: Vec<FunctionDef>,
}

impl TemplateDefV2 {
    pub fn get_function(&self, name: &str) -> Option<&FunctionDef> {
        self.functions.iter().find(|f| f.name.as_str() == name)
    }
}

#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct FunctionDef {
    #[n(0)]
    pub name: String,
    #[n(1)]
    pub kind: FunctionKind,
    /// The parameters the caller supplies, in declaration order. [`Self::kind`] carries a method's receiver and a
    /// migration's previous state, which precede them in the Rust signature.
    #[n(2)]
    pub arguments: Vec<ArgDef>,
    #[n(3)]
    pub output: Type,
}

impl FunctionDef {
    /// The method's receiver, or `None` for a function or a migration.
    pub fn receiver(&self) -> Option<Receiver> {
        match self.kind {
            FunctionKind::Method(receiver) => Some(receiver),
            FunctionKind::Function | FunctionKind::Migration { .. } => None,
        }
    }

    /// True for a component method, which the caller invokes on a component.
    pub fn has_receiver(&self) -> bool {
        self.receiver().is_some()
    }

    /// True for a method taking `&mut self`, which writes the component's state.
    pub fn is_mut(&self) -> bool {
        matches!(self.kind, FunctionKind::Method(Receiver::RefMut))
    }

    /// True for a migration, which runs only while a component moves to this template.
    pub fn is_migration(&self) -> bool {
        matches!(self.kind, FunctionKind::Migration { .. })
    }

    /// True when the engine passes a component address ahead of [`Self::arguments`]: a method receives its
    /// component, and a migration the component whose state it converts.
    pub fn takes_component(&self) -> bool {
        match self.kind {
            FunctionKind::Method(_) | FunctionKind::Migration { .. } => true,
            FunctionKind::Function => false,
        }
    }

    /// How many values a call passes: the component address if [`Self::takes_component`], then one per argument.
    pub fn num_call_args(&self) -> usize {
        usize::from(self.takes_component()) + self.arguments.len()
    }
}

/// How a template function is called.
#[derive(Debug, Clone, PartialEq, Eq, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum FunctionKind {
    /// A static function, called on the template.
    #[n(0)]
    Function,
    /// A method, called on a component of the template.
    #[n(1)]
    Method(#[n(0)] Receiver),
    /// A migration, run by `UpdateComponentTemplate` to convert a component's state from its previous template.
    #[n(2)]
    Migration {
        /// The previous template's component state, which the template loads from the component itself.
        #[n(0)]
        from: Type,
    },
}

/// How a method takes its component.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
#[cbor(index_only)]
pub enum Receiver {
    /// `&self`
    #[n(0)]
    Ref,
    /// `&mut self`
    #[n(1)]
    RefMut,
}

/// The first template definition layout. A method's receiver is its first argument, named `self` and typed
/// `Other { name: "&self" }` or `Other { name: "&mut self" }`, and `is_mut` sets the component lock.
#[derive(Debug, Clone, Default, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct TemplateDefV1 {
    #[n(0)]
    pub template_name: String,
    #[n(1)]
    pub abi_version: WasmAbiVersion,
    #[n(2)]
    pub functions: Vec<FunctionDefV1>,
}

impl From<TemplateDefV1> for TemplateDefV2 {
    fn from(def: TemplateDefV1) -> Self {
        Self {
            template_name: def.template_name,
            abi_version: def.abi_version,
            functions: def.functions.into_iter().map(Into::into).collect(),
        }
    }
}

/// A function in the [`TemplateDefV1`] layout.
#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
pub struct FunctionDefV1 {
    #[n(0)]
    pub name: String,
    #[n(1)]
    pub arguments: Vec<ArgDef>,
    #[n(2)]
    pub output: Type,
    #[n(3)]
    pub is_mut: bool,
    #[n(4)]
    pub is_migration: bool,
}

impl From<FunctionDefV1> for FunctionDef {
    /// A leading `&self` or `&mut self` argument becomes the receiver, whose mutability comes from `is_mut` because
    /// that is what sets the component lock for a V1 method. A migration's first argument, its previous state,
    /// becomes [`FunctionKind::Migration::from`]. A V1 migration with no arguments, which only a hand-built definition
    /// can have, migrates from [`Type::Unit`].
    fn from(def: FunctionDefV1) -> Self {
        let FunctionDefV1 {
            name,
            mut arguments,
            output,
            is_mut,
            is_migration,
        } = def;
        let kind = if is_migration {
            let from = if arguments.is_empty() {
                Type::Unit
            } else {
                arguments.remove(0).arg_type
            };
            FunctionKind::Migration { from }
        } else if arguments
            .first()
            .is_some_and(|arg| matches!(arg.arg_type.other(), Some("&self" | "&mut self")))
        {
            arguments.remove(0);
            FunctionKind::Method(if is_mut { Receiver::RefMut } else { Receiver::Ref })
        } else {
            FunctionKind::Function
        };
        Self {
            name,
            kind,
            arguments,
            output,
        }
    }
}

#[derive(Debug, Clone, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub struct ArgDef {
    #[n(0)]
    pub name: String,
    #[n(1)]
    pub arg_type: Type,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Encode, Decode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
#[cfg_attr(feature = "ts", derive(ts_rs::TS), ts(export))]
pub enum Type {
    #[default]
    #[n(0)]
    Unit,
    #[n(1)]
    Bool,
    #[n(2)]
    I8,
    #[n(3)]
    I16,
    #[n(4)]
    I32,
    #[n(5)]
    I64,
    #[n(6)]
    I128,
    #[n(7)]
    U8,
    #[n(8)]
    U16,
    #[n(9)]
    U32,
    #[n(10)]
    U64,
    #[n(11)]
    U128,
    #[n(12)]
    String,
    #[n(13)]
    Vec(#[n(0)] Box<Type>),
    #[n(14)]
    Tuple(#[n(0)] Vec<Type>),
    #[n(15)]
    Other {
        #[n(0)]
        name: String,
    },
    #[n(16)]
    Option(#[n(0)] Box<Type>),
}

impl Type {
    pub fn other(&self) -> Option<&str> {
        match self {
            Type::Other { name } => Some(name),
            _ => None,
        }
    }
}

#[cfg(feature = "std")]
impl std::fmt::Display for Type {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Type::Unit => write!(f, "Unit"),
            Type::Bool => write!(f, "Bool"),
            Type::I8 => write!(f, "I8"),
            Type::I16 => write!(f, "I16"),
            Type::I32 => write!(f, "I32"),
            Type::I64 => write!(f, "I64"),
            Type::I128 => write!(f, "I128"),
            Type::U8 => write!(f, "U8"),
            Type::U16 => write!(f, "U16"),
            Type::U32 => write!(f, "U32"),
            Type::U64 => write!(f, "U64"),
            Type::U128 => write!(f, "U128"),
            Type::String => write!(f, "String"),
            Type::Vec(t) => write!(f, "Vec<{}>", t),
            Type::Option(t) => write!(f, "Option<{}>", t),
            Type::Tuple(types) => {
                let type_list = types.iter().map(|t| t.to_string()).collect::<Vec<_>>().join(",");
                write!(f, "Tuple<{}>", type_list)
            },
            Type::Other { name } => write!(f, "{}", name),
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod layout_tests {
    use super::*;

    fn arg(name: &str, arg_type: Type) -> ArgDef {
        ArgDef {
            name: name.to_string(),
            arg_type,
        }
    }

    fn receiver_arg(ty: &str) -> ArgDef {
        arg("self", Type::Other { name: ty.to_string() })
    }

    fn v1_function(name: &str, arguments: Vec<ArgDef>, is_mut: bool) -> FunctionDefV1 {
        FunctionDefV1 {
            name: name.to_string(),
            arguments,
            output: Type::Unit,
            is_mut,
            is_migration: false,
        }
    }

    fn v1_migration(arguments: Vec<ArgDef>) -> FunctionDefV1 {
        FunctionDefV1 {
            name: "migrate".to_string(),
            arguments,
            output: Type::Other {
                name: "Counter".to_string(),
            },
            is_mut: false,
            is_migration: true,
        }
    }

    fn previous_state() -> Type {
        Type::Other {
            name: "CounterV1".to_string(),
        }
    }

    fn v1_def() -> VersionedTemplateDef {
        VersionedTemplateDef::V1(TemplateDefV1 {
            template_name: "Counter".to_string(),
            abi_version: 0,
            functions: vec![
                v1_function("new", vec![arg("start", Type::U64)], false),
                v1_function("get", vec![receiver_arg("&self")], false),
                v1_function("add", vec![receiver_arg("&mut self"), arg("by", Type::U64)], true),
                v1_migration(vec![arg("previous", previous_state()), arg("seed", Type::U32)]),
            ],
        })
    }

    fn assert_converted(def: &TemplateDef) {
        let new = def.get_function("new").unwrap();
        assert_eq!(new.receiver(), None);
        assert_eq!(new.arguments.len(), 1);
        assert_eq!(new.arguments[0].name, "start");

        let get = def.get_function("get").unwrap();
        assert_eq!(get.receiver(), Some(Receiver::Ref));
        assert!(get.arguments.is_empty());
        assert_eq!(get.num_call_args(), 1);

        let add = def.get_function("add").unwrap();
        assert_eq!(add.receiver(), Some(Receiver::RefMut));
        assert!(add.is_mut());
        assert_eq!(add.arguments.len(), 1);
        assert_eq!(add.arguments[0].name, "by");
        assert_eq!(add.num_call_args(), 2);

        let migrate = def.get_function("migrate").unwrap();
        assert_eq!(migrate.kind, FunctionKind::Migration { from: previous_state() });
        assert_eq!(migrate.receiver(), None);
        assert_eq!(migrate.arguments.len(), 1);
        assert_eq!(migrate.arguments[0].name, "seed");
        assert_eq!(migrate.num_call_args(), 2);
    }

    #[test]
    fn a_v1_definition_decodes_with_its_receivers_lifted_out_of_the_arguments() {
        let bytes = tari_bor::encode(&v1_def()).unwrap();
        let def = tari_bor::decode::<TemplateDef>(&bytes).unwrap();
        assert_converted(&def);
    }

    #[test]
    fn a_v1_receiver_takes_its_mutability_from_is_mut() {
        let converted = FunctionDef::from(v1_function("f", vec![receiver_arg("&self")], true));
        assert_eq!(converted.receiver(), Some(Receiver::RefMut));
    }

    #[test]
    fn a_v1_migration_stays_a_migration_whatever_its_first_argument() {
        let converted = FunctionDef::from(v1_migration(vec![receiver_arg("&mut self")]));
        assert_eq!(converted.kind, FunctionKind::Migration {
            from: Type::Other {
                name: "&mut self".to_string()
            }
        });
        assert!(converted.arguments.is_empty());

        let converted = FunctionDef::from(v1_migration(vec![]));
        assert_eq!(converted.kind, FunctionKind::Migration { from: Type::Unit });
    }

    #[test]
    fn the_current_layout_encodes_and_decodes_unchanged() {
        let bytes = tari_bor::encode(&v1_def()).unwrap();
        let def = tari_bor::decode::<TemplateDef>(&bytes).unwrap();

        let reencoded = tari_bor::encode(&def).unwrap();
        assert_converted(&tari_bor::decode::<TemplateDef>(&reencoded).unwrap());
        assert!(matches!(
            tari_bor::decode::<VersionedTemplateDef>(&reencoded).unwrap(),
            VersionedTemplateDef::V2(_)
        ));
    }

    #[cfg(feature = "serde")]
    #[test]
    fn a_v1_definition_deserializes_through_serde_as_the_current_layout() {
        let bytes = tari_bor::serde_codec::to_vec(v1_def()).unwrap();
        let def: TemplateDef = tari_bor::serde_codec::from_slice(&bytes).unwrap();
        assert_converted(&def);
    }
}

#[cfg(all(test, feature = "std"))]
mod nesting_depth_tests {
    use super::*;

    // The bound the engine applies at its trust boundaries
    // (`tari_engine_types::limits::MAX_CBOR_NESTING_DEPTH`). Named here rather than imported
    // because `tari_engine_types` depends on this crate, not the other way around.
    const MAX_NESTING_DEPTH: usize = 256;

    // `Type` is self-recursive through `Vec`/`Tuple`/`Option` and both of its decoders are derived,
    // so neither can thread a nesting bound of its own. The bound belongs to `tari_bor`'s decode
    // entry points, and these payloads are what prove it is there: without it the decoder recurses
    // once per level and the stack overflow aborts the process instead of returning an error.
    const PAST_THE_BOUND: usize = 100_000;

    // The smallest stack untrusted decode runs on: a tokio worker. Whatever the bound admits has to
    // decode within it, or the bound is not a bound.
    const WORKER_STACK_BYTES: usize = 2 * 1024 * 1024;

    // Repeats one `Type::Vec` wrapper's worth of framing, taken from a real encode so the fixture
    // follows whatever framing the codec emits.
    fn nested_payload(one_level: &[u8], innermost: &[u8], levels: usize) -> Vec<u8> {
        let prefix = &one_level[..one_level.len() - innermost.len()];
        let mut bytes = Vec::with_capacity(prefix.len() * levels + innermost.len());
        for _ in 0..levels {
            bytes.extend_from_slice(prefix);
        }
        bytes.extend_from_slice(innermost);
        bytes
    }

    // The deepest payload the bound lets through, found by asking the bound rather than by counting
    // wire levels per `Type` level — that count is the codec's business and differs between the two.
    fn deepest_payload_within_bound(build: impl Fn(usize) -> Vec<u8>) -> Vec<u8> {
        (1..=MAX_NESTING_DEPTH)
            .rev()
            .map(build)
            .find(|bytes| tari_bor::check_nesting_depth(bytes, MAX_NESTING_DEPTH).is_ok())
            .expect("no nesting level is within the bound")
    }

    fn on_a_worker_sized_stack(f: impl FnOnce() + Send + 'static) {
        std::thread::Builder::new()
            .stack_size(WORKER_STACK_BYTES)
            .spawn(f)
            .unwrap()
            .join()
            .unwrap();
    }

    fn minicbor_payload(levels: usize) -> Vec<u8> {
        let innermost = tari_bor::encode(&Type::Unit).unwrap();
        let one_level = tari_bor::encode(&Type::Vec(Box::new(Type::Unit))).unwrap();
        nested_payload(&one_level, &innermost, levels)
    }

    #[test]
    fn deeply_nested_type_is_rejected_by_the_minicbor_decoder() {
        let bytes = minicbor_payload(PAST_THE_BOUND);
        assert!(tari_bor::decode_with_max_depth::<Type>(&bytes, MAX_NESTING_DEPTH).is_err());
    }

    #[test]
    fn minicbor_decode_at_the_bound_fits_a_worker_stack() {
        let bytes = deepest_payload_within_bound(minicbor_payload);
        on_a_worker_sized_stack(move || {
            tari_bor::decode_with_max_depth::<Type>(&bytes, MAX_NESTING_DEPTH).unwrap();
        });
    }

    #[cfg(feature = "serde")]
    mod serde_route {
        use tari_bor::serde_codec;

        use super::*;

        fn serde_payload(levels: usize) -> Vec<u8> {
            let innermost = serde_codec::to_vec(Type::Unit).unwrap();
            let one_level = serde_codec::to_vec(Type::Vec(Box::new(Type::Unit))).unwrap();
            nested_payload(&one_level, &innermost, levels)
        }

        #[test]
        fn deeply_nested_type_is_rejected_by_the_serde_decoder() {
            let bytes = serde_payload(PAST_THE_BOUND);
            assert!(serde_codec::from_slice_with_max_depth::<Type>(&bytes, MAX_NESTING_DEPTH).is_err());
        }

        #[test]
        fn serde_decode_at_the_bound_fits_a_worker_stack() {
            let bytes = deepest_payload_within_bound(serde_payload);
            on_a_worker_sized_stack(move || {
                serde_codec::from_slice_with_max_depth::<Type>(&bytes, MAX_NESTING_DEPTH).unwrap();
            });
        }
    }
}
