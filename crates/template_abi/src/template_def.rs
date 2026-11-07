//  Copyright 2022. The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

use minicbor::{CborLen, Decode, Encode};

use crate::{
    rust::{boxed::Box, string::String, vec::Vec},
    version::WasmAbiVersion,
};

/// A template's definition, as the `#[template]` macro embeds it in the template binary.
///
/// The enum index is the layout version and is part of the encoding, so a future layout is a new variant at the next
/// index. Index 0 is the first layout, which the engine does not load: decoding it fails with an error telling the
/// author to rebuild the template.
#[derive(Debug, Clone, Encode, CborLen)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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
        let full_len = data_len
            .checked_add(WASM_PTR_SIZE)
            .ok_or_else(|| tari_bor::BorError::new("Template definition length overflow".to_string()))?;
        let full_len_u32 = u32::try_from(full_len)
            .map_err(|_| tari_bor::BorError::new("Template definition length exceeds u32".to_string()))?;
        let mut buf = Vec::with_capacity(full_len);
        let writer = &mut buf;
        // length prefix
        writer
            .write_all(&full_len_u32.to_le_bytes())
            .map_err(|e| tari_bor::BorError::new(e.to_string()))?;
        encode_into_writer(self, writer)?;
        Ok(buf)
    }
}

/// The enum index of the first template definition layout, which the engine refuses to load.
const V1_LAYOUT_INDEX: u32 = 0;

impl<'b, C> Decode<'b, C> for TemplateDef {
    fn decode(d: &mut minicbor::Decoder<'b>, ctx: &mut C) -> Result<Self, minicbor::decode::Error> {
        let mut probe = d.probe();
        probe.array()?;
        if probe.u32()? == V1_LAYOUT_INDEX {
            return Err(minicbor::decode::Error::message(
                "the template definition uses the V1 layout, which is not supported. Rebuild the template with \
                 tari_template_lib 0.38 or later",
            ));
        }
        DecodedTemplateDef::decode(d, ctx).map(|DecodedTemplateDef::V2(def)| TemplateDef::V2(def))
    }
}

/// The derived decoder for [`TemplateDef`]'s layout. [`TemplateDef`] decodes by hand so that a V1 definition fails
/// with an error naming the layout.
#[derive(Decode)]
enum DecodedTemplateDef {
    #[n(1)]
    V2(#[n(0)] TemplateDefV2),
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
                f.write_str("Tuple<")?;
                for (i, t) in types.iter().enumerate() {
                    if i > 0 {
                        f.write_str(",")?;
                    }
                    write!(f, "{t}")?;
                }
                f.write_str(">")
            },
            Type::Other { name } => write!(f, "{}", name),
        }
    }
}

#[cfg(all(test, feature = "std"))]
mod layout_tests {
    use super::*;

    fn def() -> TemplateDef {
        TemplateDef::V2(TemplateDefV2 {
            template_name: "Counter".to_string(),
            abi_version: 0,
            functions: vec![
                FunctionDef {
                    name: "new".to_string(),
                    kind: FunctionKind::Function,
                    arguments: vec![ArgDef {
                        name: "start".to_string(),
                        arg_type: Type::U64,
                    }],
                    output: Type::Other {
                        name: "Component".to_string(),
                    },
                },
                FunctionDef {
                    name: "add".to_string(),
                    kind: FunctionKind::Method(Receiver::RefMut),
                    arguments: vec![ArgDef {
                        name: "by".to_string(),
                        arg_type: Type::U64,
                    }],
                    output: Type::Unit,
                },
                FunctionDef {
                    name: "migrate".to_string(),
                    kind: FunctionKind::Migration {
                        from: Type::Other {
                            name: "CounterV1".to_string(),
                        },
                    },
                    arguments: vec![],
                    output: Type::Other {
                        name: "Counter".to_string(),
                    },
                },
            ],
        })
    }

    fn assert_same(a: &TemplateDef, b: &TemplateDef) {
        assert_eq!(a.template_name(), b.template_name());
        assert_eq!(a.abi_version(), b.abi_version());
        assert_eq!(a.functions().len(), b.functions().len());
        for (a, b) in a.functions().iter().zip(b.functions()) {
            assert_eq!(a.name, b.name);
            assert_eq!(a.kind, b.kind);
            assert_eq!(a.output, b.output);
            assert_eq!(
                a.arguments
                    .iter()
                    .map(|arg| (&arg.name, &arg.arg_type))
                    .collect::<Vec<_>>(),
                b.arguments
                    .iter()
                    .map(|arg| (&arg.name, &arg.arg_type))
                    .collect::<Vec<_>>()
            );
        }
    }

    #[test]
    fn the_v2_layout_encodes_at_index_1_and_decodes_unchanged() {
        let bytes = tari_bor::encode(&def()).unwrap();
        // `[1, [..]]`: a two-element array holding the variant index, then the variant's fields.
        assert_eq!(bytes[..2], [0x82, 0x01]);
        assert_same(&tari_bor::decode::<TemplateDef>(&bytes).unwrap(), &def());
    }

    #[test]
    fn a_v1_definition_is_rejected_with_a_rebuild_message() {
        // `[0, [["Buggy", 0, []]]]`: the V1 layout's index and a V1 definition with no functions.
        let bytes = [0x82, 0x00, 0x81, 0x83, 0x65, b'B', b'u', b'g', b'g', b'y', 0x00, 0x80];
        let err = tari_bor::decode::<TemplateDef>(&bytes).unwrap_err();
        assert!(err.to_string().contains("V1 layout"), "{err}");
    }

    #[test]
    fn an_unknown_layout_is_rejected() {
        let mut bytes = tari_bor::encode(&def()).unwrap();
        bytes[1] = 0x02;
        assert!(tari_bor::decode::<TemplateDef>(&bytes).is_err());
    }

    #[cfg(feature = "serde")]
    #[test]
    fn the_v2_layout_round_trips_through_serde() {
        let bytes = tari_bor::serde_codec::to_vec(def()).unwrap();
        let decoded: TemplateDef = tari_bor::serde_codec::from_slice(&bytes).unwrap();
        assert_same(&decoded, &def());
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
