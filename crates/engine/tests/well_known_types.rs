//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

//! The `#[template]` macro tags a `tari_template_lib` type in the template definition by what it is, and the template
//! fails to build when a type with that name is some other type.

use tari_engine::wasm::WasmModule;
use tari_template_abi::{Type, WellKnownType};
use tari_template_test_tooling::compile::compile_template;

const TEMPLATE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/templates/well_known_types");

fn well_known(kind: WellKnownType) -> Type {
    Type::WellKnown(kind)
}

#[test]
fn the_definition_tags_the_library_types() {
    let module = compile_template(TEMPLATE, &[]).unwrap();
    let def = WasmModule::validate_code(module.code()).unwrap();

    let take = def.get_function("take").unwrap();
    let types = take.arguments.iter().map(|a| a.arg_type.clone()).collect::<Vec<_>>();
    assert_eq!(types, vec![
        well_known(WellKnownType::Bucket),
        Type::Vec(Box::new(well_known(WellKnownType::Amount))),
        Type::Option(Box::new(well_known(WellKnownType::TemplateAddress))),
        Type::Tuple(vec![
            well_known(WellKnownType::ComponentAddress),
            well_known(WellKnownType::NonFungibleId)
        ]),
    ]);
    assert_eq!(take.output, well_known(WellKnownType::Amount));
}

/// The same template, with `Bucket` naming a type the template defines in another module.
#[test]
fn a_lookalike_of_a_well_known_type_fails_to_build() {
    assert!(compile_template(TEMPLATE, &["lookalike"]).is_err());
}
