//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_ootle_transaction::{Epoch, Transaction, args};
use tari_template_abi::TemplateDef;
use tari_template_lib::types::ComponentAddress;
use tari_template_test_tooling::{TemplateTest, compile::compile_template};

const CRATE_PATH: &str = env!("CARGO_MANIFEST_DIR");

#[derive(minicbor::Encode)]
struct HostOrder {
    #[n(0)]
    price: u64,
    #[n(1)]
    qty: u64,
}

#[derive(minicbor::Encode)]
enum HostSide {
    #[n(0)]
    Buy,
    #[n(1)]
    Sell,
}

#[derive(minicbor::Encode)]
struct HostDiscount {
    #[n(0)]
    bps: u64,
}

/// Templates using module-local structs/enums, `use super::...` imports, wildcard/destructuring parameter patterns,
/// and raw `r#` identifiers compile to WASM, pass `PublishTemplate` ABI validation, and dispatch end to end.
#[test]
fn template_with_module_types_super_imports_patterns_and_raw_idents_publishes_and_executes() {
    let mut test = TemplateTest::new(CRATE_PATH, ["tests/templates/macro_scope_probe"]);

    let loaded = test.get_module("Match");
    let TemplateDef::V2(def) = loaded.template_def();
    assert_eq!(def.template_name, "Match");

    let submit_def = def.functions.iter().find(|f| f.name == "submit").unwrap();
    let submit_arg_names = submit_def.arguments.iter().map(|a| a.name.as_str()).collect::<Vec<_>>();
    assert_eq!(submit_arg_names, vec!["order", "side", "discount", "_", "_"]);

    let match_def = def.functions.iter().find(|f| f.name == "match").unwrap();
    let match_arg_names = match_def.arguments.iter().map(|a| a.name.as_str()).collect::<Vec<_>>();
    assert_eq!(match_arg_names, vec!["type"]);

    // `PublishTemplate` runs consensus `WasmModule::validate` over `_TARI_TEMPLATE_DEF` and the `Match_main` export.
    let (payer, payer_proof, payer_sk) = test.create_funded_account();
    let compiled = compile_template("tests/templates/macro_scope_probe", &[]).unwrap();
    test.execute_expect_success(
        Transaction::builder_localnet(Epoch(1))
            .with_auto_fill_inputs()
            .pay_fee_from_component(payer, 250_000u64)
            .publish_template(compiled.into_code())
            .build_and_seal(&payer_sk),
        vec![payer_proof],
    );

    let component: ComponentAddress = test.call_function("Match", "new", args![], vec![]);

    let after_buy: u64 = test.call_method(
        component,
        "submit",
        args![
            HostOrder { price: 25, qty: 4 },
            HostSide::Buy,
            HostDiscount { bps: 10 },
            999u64,
            (3u64, 7u64)
        ],
        vec![],
    );
    assert_eq!(after_buy, 100);

    let matches_hundred: bool = test.call_method(component, "match", args![100u64], vec![]);
    assert!(matches_hundred);

    let after_sell: u64 = test.call_method(
        component,
        "submit",
        args![
            HostOrder { price: 10, qty: 3 },
            HostSide::Sell,
            HostDiscount { bps: 5 },
            0u64,
            (2u64, 3u64)
        ],
        vec![],
    );
    assert_eq!(after_sell, 70);

    let matches_seventy: bool = test.call_method(component, "match", args![70u64], vec![]);
    assert!(matches_seventy);
}
