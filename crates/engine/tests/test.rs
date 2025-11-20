//   Copyright 2022. The Tari Project
//
//   Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//   following conditions are met:
//
//   1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//   disclaimer.
//
//   2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//   following disclaimer in the documentation and/or other materials provided with the distribution.
//
//   3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//   products derived from this software without specific prior written permission.
//
//   THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//   INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//   DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//   SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//   SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//   WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//   USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.
use std::iter;

use serde::{Deserialize, Serialize};
use tari_engine::{
    template::{TemplateLoaderError, TemplateModuleLoader},
    wasm::{compile::compile_template, WasmExecutionError},
};
use tari_engine_types::{
    commit_result::{FinalizeResult, RejectReason},
    substate::SubstateId,
    virtual_substate::{VirtualSubstate, VirtualSubstateId},
};
use tari_ootle_common_types::substate_type::SubstateType;
use tari_template_builtin::{ACCOUNT_TEMPLATE_ADDRESS, NFT_FAUCET_TEMPLATE_ADDRESS};
use tari_template_lib::{
    constants::XTR,
    models::{ComponentAddress, NonFungible, NonFungibleAddress, ResourceAddress},
    types::{crypto::RistrettoPublicKeyBytes, Amount, TemplateAddress},
};
use tari_template_test_tooling::{support::assert_error::assert_reject_reason, TemplateTest};
use tari_transaction::{args, call_args, Transaction};
use tari_transaction_manifest::ManifestValue;
use tari_utilities::hex::to_hex;
use wasmer::ExportError;

#[test]
fn test_hello_world() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/hello_world"]);
    let result: String = template_test.call_function("HelloWorld", "greet", call_args![], vec![]);

    assert_eq!(result, "Hello World!");
}

#[test]
fn test_state() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/state"]);

    // constructor
    let component_address1: ComponentAddress = template_test.call_function("State", "new", call_args![], vec![]);
    template_test.assert_calls(&[
        "emit_log",
        "component_invoke",
        "set_last_instruction_output",
        "finalize",
    ]);

    let component_address2: ComponentAddress = template_test.call_function("State", "new", call_args![], vec![]);
    assert_ne!(component_address1, component_address2);

    let store = template_test.read_only_state_store();
    let component = store.get_component(component_address1).unwrap();
    assert_eq!(component.module_name, "State");

    let component = store.get_component(component_address2).unwrap();
    assert_eq!(component.owner_key, Some(template_test.to_public_key_bytes()));
    assert_eq!(component.module_name, "State");

    // call the "set" method to update the instance value
    let new_value = 20_u32;
    template_test.call_method::<()>(component_address2, "set", call_args![new_value], vec![]);

    // call the "get" method to get the current value
    let value: u32 = template_test.call_method(component_address2, "get", call_args![], vec![]);

    assert_eq!(value, new_value);
}

#[test]
fn state_create_multiple_in_one_call() {
    let mut template_test = TemplateTest::new(["tests/templates/state"]);

    // constructor
    template_test.call_function::<()>("State", "create_multiple", call_args![10u32], vec![]);

    let template_address = template_test.get_template_address("State");
    let mut count = 0usize;
    template_test
        .read_only_state_store()
        .with_substates(|_, s| {
            if s.substate_value()
                .component()
                .filter(|a| a.template_address == template_address)
                .is_some()
            {
                count += 1;
            }
        })
        .unwrap();
    assert_eq!(count, 10);
}

#[test]
fn test_composed() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/state", "tests/templates/hello_world"]);

    let module = template_test.get_module("HelloWorld");
    let functions = module
        .template_def()
        .functions()
        .iter()
        .map(|f| f.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(functions, vec!["greet", "new", "custom_greeting"]);

    let module = template_test.get_module("State");
    let functions = module
        .template_def()
        .functions()
        .iter()
        .map(|f| f.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(functions, vec!["new", "create_multiple", "restricted", "set", "get"]);

    let component_state: ComponentAddress = template_test.call_function("State", "new", call_args![], vec![]);
    let component_hw: ComponentAddress = template_test.call_function("HelloWorld", "new", call_args!["أهلا"], vec![]);

    let result: String = template_test.call_method(component_hw, "custom_greeting", call_args!["Wasm"], vec![]);
    assert_eq!(result, "أهلا Wasm!");

    // call the "set" method to update the instance value
    let new_value = 20_u32;
    template_test.call_method::<()>(component_state, "set", call_args![new_value], vec![]);

    // call the "get" method to get the current value
    let value: u32 = template_test.call_method(component_state, "get", call_args![], vec![]);

    assert_eq!(value, new_value);
}

#[test]
fn test_buggy_template() {
    // Uncomment the following lines to print the ABI bytes
    // let bytes = tari_bor::encode_with_len(&tari_template_abi::TemplateDef::V1(tari_template_abi::TemplateDefV1 {
    //     template_name: "Buggy".to_string(),
    //     tari_version: tari_template_abi::version::MINIMUM_SUPPORTED_WASM_ABI_VERSION.to_string(),
    //     functions: vec![],
    // }));
    // println!("pub static _ABI_TEMPLATE_DEF: [u8; {}] = [", bytes.len());
    // for chunk in bytes.chunks(16) {
    //     print!("    ");
    //     for byte in chunk {
    //         print!("{}, ", byte);
    //     }
    //     println!();
    // }
    // println!("];");

    let err = compile_template("tests/templates/buggy", &["return_null_abi"])
        .unwrap()
        .load_template()
        .unwrap_err();
    match err {
        // The ptr location is non-zero, and the pointer reads a large length that is out of range
        TemplateLoaderError::WasmModuleError(WasmExecutionError::MemoryPointerOutOfRange { .. }) => {},
        // The ptr location is zero, so the decode fails
        TemplateLoaderError::WasmModuleError(WasmExecutionError::AbiDecodeError { .. }) => {},
        _ => panic!("Unexpected error: {:?}", err),
    }

    let err = compile_template("tests/templates/buggy", &["unexpected_export_function"])
        .unwrap()
        .load_template()
        .unwrap_err();
    assert!(matches!(
        err,
        TemplateLoaderError::WasmModuleError(WasmExecutionError::UnexpectedAbiFunction { .. })
    ));

    let err = compile_template("tests/templates/buggy", &["return_empty_abi"])
        .unwrap()
        .load_template()
        .unwrap_err();
    assert!(matches!(
        err,
        TemplateLoaderError::WasmModuleError(WasmExecutionError::AbiDecodeError(_))
    ));

    let err = compile_template("tests/templates/buggy", &["no_template_def"])
        .unwrap()
        .load_template()
        .unwrap_err();

    assert!(matches!(
        err,
        TemplateLoaderError::WasmModuleError(WasmExecutionError::ExportError(ExportError::Missing(_)))
    ));
}

#[test]
fn test_private_function() {
    // instantiate the counter
    let mut template_test = TemplateTest::new(vec!["tests/templates/private_function"]);

    // check that the private method and function are not exported
    let module = template_test.get_module("PrivateCounter");
    let functions = module
        .template_def()
        .functions()
        .iter()
        .map(|f| f.name.as_str())
        .collect::<Vec<_>>();
    assert_eq!(functions, vec!["new", "get", "increase"]);

    // check that public methods can still internally call private ones
    let component: ComponentAddress = template_test.call_function("PrivateCounter", "new", call_args![], vec![]);
    template_test.call_method::<()>(component, "increase", call_args![], vec![]);
    let value: u32 = template_test.call_method(component, "get", call_args![], vec![]);
    assert_eq!(value, 1);
}

#[test]
fn test_engine_errors() {
    // instantiate the counter
    let mut test = TemplateTest::new(vec!["tests/templates/errors"]);

    // check that public methods can still internally call private ones
    let result = test
        .try_execute(
            Transaction::builder()
                .call_function(test.get_template_address("Errors"), "invalid_engine_call", args![])
                .build_and_seal(&Default::default()),
            vec![],
        )
        .unwrap();

    let RejectReason::ExecutionFailure(reason) = result.finalize.result.any_reject().unwrap() else {
        panic!(
            "Unexpected transaction reject reason: {}",
            result.finalize.result.fee_reject().unwrap()
        );
    };

    // Check that the engine error is captured in the execution result rather than the WASM panic message (Panic! Engine
    // call returned null for op VaultInvoke)
    assert_eq!(
        reason,
        "Runtime error: Substate 'resource_7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b7b' not \
         found or is not a transaction input"
    );
}

#[test]
fn test_tuples() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/tuples"]);

    // check that the ABI is valid
    let module = template_test.get_module("Tuple");
    // the "new" constructor returns a tuple (Component, String)
    let fn_new = module.find_func_by_name("new").unwrap();
    assert_eq!(fn_new.output.to_string(), "Tuple<Other { name: \"Component\" },String>");
    // the "get" method returns a tuple (String, u32)
    let fn_get = module.find_func_by_name("get").unwrap();
    assert_eq!(fn_get.output.to_string(), "Tuple<String,U32>");
    // the "set" method accepts a tuple (String, u32) as argument
    let fn_set = module.find_func_by_name("set").unwrap();
    assert_eq!(fn_set.arguments[1].arg_type.to_string(), "Tuple<String,U32>");

    // tuples returned in a constructor
    let (component_id, message): (ComponentAddress, String) =
        template_test.call_function("Tuple", "new", call_args![], vec![]);
    assert_eq!(message, "Hello World!");

    // tuples returned in a method
    let (message, number): (String, u32) = template_test.call_method(component_id, "get", call_args![], vec![]);
    assert_eq!(message, "Hello World!");
    assert_eq!(number, 0);

    // tuples passed as arguments to methods
    let new_value = ("New String".to_string(), 1);
    template_test.call_method::<()>(component_id, "set", call_args![new_value], vec![]);
    // check that the component state was actually updated
    let value: (String, u32) = template_test.call_method(component_id, "get", call_args![], vec![]);
    assert_eq!(value, new_value);
}

#[test]
fn test_get_template_address() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/component_manager"]);
    let (account, _, _) = template_test.create_empty_account();

    let addr: TemplateAddress = template_test.call_function(
        "ComponentManagerTest",
        "get_template_address_for_component",
        call_args![account],
        vec![],
    );
    assert_eq!(addr, template_test.get_template_address("Account"));
}

#[test]
fn test_caller_context() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/caller_context"]);

    // tuples returned in a regular function
    let component: ComponentAddress = template_test.call_function("CallerContextTest", "create", call_args![], vec![]);
    let value: RistrettoPublicKeyBytes = template_test.call_method(component, "caller_pub_key", call_args![], vec![]);
    assert_eq!(
        to_hex(value.as_bytes()),
        "d884dd886cc7464402a04920485aebe6dd657b98072de655c46ec6179a52cd0d"
    );
}

#[test]
fn test_random() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/random"]);
    let component_address: ComponentAddress = template_test.call_function("RandomTest", "create", args![], vec![]);
    let value: u32 = template_test.call_method(component_address, "get_random", call_args![], vec![]);
    assert_ne!(value, 0);

    let value: Vec<u8> = template_test.call_method(component_address, "get_random_bytes", call_args![], vec![]);
    assert_eq!(value.len(), 32);
    assert_ne!(value, vec![0; 32]);

    let value: Vec<u8> = template_test.call_method(component_address, "get_random_long_bytes", call_args![], vec![]);
    assert_eq!(value.len(), 300);
    assert_ne!(value, vec![0; 300]);
}

#[test]
fn test_errors_on_infinite_loop() {
    let mut test = TemplateTest::new(vec!["tests/templates/infinity_loop"]);
    let reason = test.execute_expect_failure(
        Transaction::builder()
            .call_function(test.get_template_address("InfinityLoopTest"), "infinity_loop", args![])
            .build_and_seal(test.secret_key()),
        vec![],
    );
    // Transaction failed: Execution failure: RuntimeError: unreachable\n    at tari_free (<module>[327]:0x2390c)
    assert_reject_reason(reason, wasmer::RuntimeError::new("unreachable"))
}

mod errors {
    use tari_transaction::Instruction;

    use super::*;

    #[test]
    fn panic() {
        let mut template_test = TemplateTest::new(vec!["tests/templates/errors"]);

        let result = template_test
            .try_execute_instructions(
                vec![],
                vec![Instruction::CallFunction {
                    address: template_test.get_template_address("Errors"),
                    function: "panic".try_into().unwrap(),
                    args: args![],
                }],
                vec![],
            )
            .unwrap();
        match result.finalize.result.any_reject().unwrap() {
            RejectReason::ExecutionFailure(message) => {
                assert_eq!(
                    message,
                    "Panic! This error message should be included in the execution result"
                );
            },
            reason => panic!("Unexpected transaction reject reason: {}", reason),
        }
    }

    #[test]
    fn invalid_args() {
        let mut template_test = TemplateTest::new(vec!["tests/templates/errors"]);

        let text = "this isn't an amount";
        let result = template_test
            .try_execute_instructions(
                vec![],
                vec![Instruction::CallFunction {
                    address: template_test.get_template_address("Errors"),
                    function: "please_pass_invalid_args".try_into().unwrap(),
                    args: call_args![text],
                }],
                vec![],
            )
            .unwrap();
        println!("{:?}", result.finalize.result);
        match result.finalize.result.any_reject().unwrap() {
            RejectReason::ExecutionFailure(message) => {
                assert!(
                    message.starts_with(
                        "Panic! failed to decode argument at position 0 \
                         (tari_template_lib_types::amount::amount::Amount) for function 'please_pass_invalid_args':"
                    ),
                    "Got a different error: {}",
                    message
                );
            },
            reason => panic!("Unexpected failure reason: {}", reason),
        }
    }
}

mod consensus {
    use super::*;

    #[test]
    fn current_epoch() {
        let mut template_test = TemplateTest::new(vec!["tests/templates/consensus"]);

        // the default value for a current epoch in the mocks is 0
        let result: u64 = template_test.call_function("TestConsensus", "current_epoch", args![], vec![]);
        assert_eq!(result, 0);

        // set the value of current epoch to "1" and call the template function again to check that it reads the new
        // value
        template_test.set_virtual_substate(VirtualSubstateId::CurrentEpoch, VirtualSubstate::CurrentEpoch(1));
        let result: u64 = template_test.call_function("TestConsensus", "current_epoch", args![], vec![]);
        assert_eq!(result, 1);
    }
}

mod fungible {
    use tari_template_lib::types::Amount;
    use tari_transaction::Instruction;

    use super::*;

    #[test]
    fn fungible_mint_and_burn() {
        let mut template_test = TemplateTest::new(Vec::<&str>::new());

        let faucet_template = template_test.get_template_address("TestFaucet");

        let initial_supply = Amount::from(1_000_000_000_000u64);
        template_test
            .execute_and_commit(
                vec![Instruction::CallFunction {
                    address: faucet_template,
                    function: "mint".try_into().unwrap(),
                    args: call_args![initial_supply],
                }],
                vec![],
            )
            .unwrap();

        let faucet_component = template_test
            .get_previous_output_address(SubstateType::Component)
            .as_component_address()
            .unwrap();

        let total_supply: Amount = template_test.call_method(faucet_component, "total_supply", args![], vec![]);

        assert_eq!(total_supply, initial_supply);

        let owner_proof = template_test.owner_proof();
        let result = template_test.build_and_execute(
            Transaction::builder()
                .call_method(faucet_component, "burn_coins", args![500])
                .call_method(faucet_component, "total_supply", args![]),
            vec![owner_proof.clone()],
        );
        result.expect_success();

        assert_eq!(
            result.finalize.execution_results[1].decode::<Amount>().unwrap(),
            initial_supply - Amount::from(500)
        );

        let result = template_test.build_and_execute(
            Transaction::builder()
                .call_method(faucet_component, "burn_coins", args![
                    initial_supply - Amount::from(500)
                ])
                .call_method(faucet_component, "total_supply", args![]),
            vec![owner_proof],
        );
        result.expect_success();

        assert_eq!(
            result.finalize.execution_results[1].decode::<Amount>().unwrap(),
            Amount::zero()
        );

        template_test
            .build_and_execute(
                Transaction::builder().call_method(faucet_component, "burn_coins", args![1]),
                vec![],
            )
            .expect_failure();
    }
}

mod basic_nft {
    use tari_template_lib::types::Amount;

    use super::*;

    fn setup() -> (
        TemplateTest,
        (ComponentAddress, NonFungibleAddress),
        ComponentAddress,
        SubstateId,
    ) {
        let mut template_test = TemplateTest::new(vec!["tests/templates/nft/basic_nft"]);

        let (account_address, owner_token, _) = template_test.create_funded_account();
        let nft_component: ComponentAddress = template_test.call_function("SparkleNft", "new", args![], vec![]);

        let nft_resx = template_test.get_previous_output_address(SubstateType::Resource);

        // TODO: cleanup
        (template_test, (account_address, owner_token), nft_component, nft_resx)
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn create_resource_mint_and_deposit() {
        let (mut template_test, (account_address, account_owner), nft_component, nft_resx) = setup();

        let vars = vec![
            ("account", account_address.into()),
            ("nft", nft_component.into()),
            ("nft_resx", nft_resx.into()),
        ];

        let total_supply: Amount = template_test.call_method(nft_component, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 4);

        let result = template_test
            .execute_and_commit_manifest(
                r#"
            let account = var!["account"];
            let sparkle_nft = var!["nft"];

            let nft_bucket = sparkle_nft.mint();
            account.deposit(nft_bucket);
        "#,
                vars.clone(),
                vec![account_owner],
            )
            .unwrap();

        let diff = result.finalize.result.expect("execution failed");

        // Resource is changed
        assert_eq!(diff.down_iter().filter(|(addr, _)| addr.is_resource()).count(), 1);
        assert_eq!(diff.up_iter().filter(|(addr, _)| addr.is_resource()).count(), 1);

        // NFT and account components changed
        assert_eq!(diff.down_iter().filter(|(addr, _)| addr.is_component()).count(), 1);
        assert_eq!(diff.up_iter().filter(|(addr, _)| addr.is_component()).count(), 1);

        // One new vault created
        assert_eq!(diff.down_iter().filter(|(addr, _)| addr.is_vault()).count(), 0);
        assert_eq!(diff.up_iter().filter(|(addr, _)| addr.is_vault()).count(), 1);

        // One new NFT minted
        assert_eq!(diff.down_iter().filter(|(addr, _)| addr.is_non_fungible()).count(), 0);
        assert_eq!(diff.up_iter().filter(|(addr, _)| addr.is_non_fungible()).count(), 1);

        let total_supply: Amount = template_test.call_method(nft_component, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 5);

        let result = template_test
            .execute_and_commit_manifest(
                r#"
            let account = var!["account"];
            let sparkle_nft = var!["nft"];

            let nft_bucket = sparkle_nft.withdraw_all();
            account.deposit(nft_bucket);
            sparkle_nft.inner_vault_balance();

            let nft_resx = var!["nft_resx"];
            account.balance(nft_resx);
            sparkle_nft.total_supply();
        "#,
                vars,
                vec![],
            )
            .unwrap();
        result.finalize.result.expect("execution failed");
        // sparkle_nft.inner_vault_balance()
        assert_eq!(result.finalize.execution_results[3].decode::<Amount>().unwrap(), 0);
        // account.balance(nft_resx)
        assert_eq!(result.finalize.execution_results[4].decode::<Amount>().unwrap(), 5);
        // sparkle_nft.total_supply()
        assert_eq!(result.finalize.execution_results[5].decode::<Amount>().unwrap(), 5);
    }

    #[test]
    fn change_nft_mutable_data() {
        let (mut template_test, (account_address, account_owner), nft_component, _nft_resx) = setup();

        let total_supply: Amount = template_test.call_method(nft_component, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 4);

        let vars = [("account", account_address.into()), ("nft", nft_component.into())];

        let result = template_test
            .execute_and_commit_manifest(
                r#"
            let account = var!["account"];
            let sparkle_nft = var!["nft"];

            let nft_bucket = sparkle_nft.mint();
            account.deposit(nft_bucket);
        "#,
                vars,
                vec![account_owner],
            )
            .unwrap();

        let diff = result.finalize.result.expect("execution failed");
        let (_, state) = diff.up_iter().find(|(addr, _)| addr.is_non_fungible()).unwrap();

        #[derive(Debug, Clone, Serialize, Deserialize)]
        pub struct Sparkle {
            pub brightness: u32,
        }

        let sparkle = state
            .substate_value()
            .non_fungible()
            .unwrap()
            .contents()
            .unwrap()
            .decode_mutable_data::<Sparkle>()
            .unwrap();
        assert_eq!(sparkle.brightness, 0);

        let substate_addr = template_test.get_previous_output_address(SubstateType::NonFungible);
        let nft_addr = substate_addr.as_non_fungible_address().unwrap();
        let vars = [
            ("account", account_address.into()),
            ("nft", nft_component.into()),
            ("nft_resx", (*nft_addr.resource_address()).into()),
            (
                "nft_id",
                ManifestValue::NonFungibleId(substate_addr.as_non_fungible_address().unwrap().id().clone()),
            ),
        ];

        template_test
            .execute_and_commit_manifest(
                r#"
            let sparkle_nft = var!["nft"];
            let sparkle_nft_id = var!["nft_id"];
            sparkle_nft.inc_brightness(sparkle_nft_id, 10u32);
        "#,
                vars.clone(),
                vec![],
            )
            .unwrap();

        let nft = template_test
            .read_only_state_store()
            .get_substate(&substate_addr)
            .unwrap()
            .into_substate_value()
            .into_non_fungible()
            .unwrap();

        assert_eq!(
            nft.contents()
                .unwrap()
                .decode_mutable_data::<Sparkle>()
                .unwrap()
                .brightness,
            10
        );

        let err = template_test
            .execute_and_commit_manifest(
                r#"
            let sparkle_nft = var!["nft"];
            let sparkle_nft_id = var!["nft_id"];
            sparkle_nft.dec_brightness(sparkle_nft_id, 11u32);
        "#,
                vars,
                vec![],
            )
            .unwrap_err();

        assert!(err.to_string().contains("Not enough brightness remaining"));
    }

    #[test]
    fn mint_specific_id() {
        let (mut template_test, (account_address, account_owner), nft_component, nft_resx) = setup();

        let vars = vec![
            ("account", account_address.into()),
            ("nft", nft_component.into()),
            ("nft_resx", nft_resx.into()),
        ];

        let total_supply: Amount = template_test.call_method(nft_component, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 4);

        let result = template_test
            .execute_and_commit_manifest(
                r#"
            let account = var!["account"];
            let sparkle_nft = var!["nft"];

            let nft_bucket = sparkle_nft.mint_specific(NonFungibleId("SpecialNft"));
            account.deposit(nft_bucket);

            let nft_bucket = sparkle_nft.mint_specific(NonFungibleId(123u32));
            account.deposit(nft_bucket);

            let nft_bucket = sparkle_nft.mint_specific(NonFungibleId(456u64));
            account.deposit(nft_bucket);

            let nft_bucket = sparkle_nft.mint_specific(NonFungibleId(b"this will be interpreted as uuid"));
            account.deposit(nft_bucket);

            sparkle_nft.total_supply();
        "#,
                vars.clone(),
                vec![account_owner],
            )
            .unwrap();

        let diff = result.finalize.result.expect("execution failed");
        let nfts = diff
            .up_iter()
            .filter_map(|(a, _)| match a {
                SubstateId::NonFungible(address) => Some(address.id()),
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(
            nfts.iter()
                .filter(|n| n.to_canonical_string() == "str_SpecialNft")
                .count(),
            1
        );
        assert_eq!(nfts.iter().filter(|n| n.to_canonical_string() == "u32_123").count(), 1);
        assert_eq!(nfts.iter().filter(|n| n.to_canonical_string() == "u64_456").count(), 1);
        assert_eq!(
            nfts.iter()
                .filter(|n| n.to_canonical_string() ==
                    "uuid_746869732077696c6c20626520696e7465727072657465642061732075756964")
                .count(),
            1
        );
        assert_eq!(nfts.len(), 4);
        assert_eq!(result.finalize.execution_results[12].decode::<Amount>().unwrap(), 8);

        // Try mint 2 nfts with the same id in a single transaction - should fail
        template_test
            .execute_and_commit_manifest(
                r#"
            let account = var!["account"];
            let sparkle_nft = var!["nft"];

            let nft_bucket1 = sparkle_nft.mint_specific(NonFungibleId("Duplicate"));
            let nft_bucket2 = sparkle_nft.mint_specific(NonFungibleId("Duplicate"));
            account.deposit(nft_bucket1);
            account.deposit(nft_bucket2);
        "#,
                vars,
                vec![],
            )
            .unwrap_err();
    }

    #[test]
    fn burn_nft() {
        let (mut template_test, (account_address, account_owner), nft_component, nft_resx) = setup();

        let vars = vec![
            ("account", account_address.into()),
            ("nft", nft_component.into()),
            ("nft_resx", nft_resx.into()),
        ];

        let total_supply: Amount = template_test.call_method(nft_component, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 4);

        template_test
            .execute_and_commit_manifest(
                r#"
            let account = var!["account"];
            let sparkle_nft = var!["nft"];

            let nft_bucket = sparkle_nft.mint_specific(NonFungibleId("Burn!"));
            account.deposit(nft_bucket);
        "#,
                vars.clone(),
                vec![account_owner.clone()],
            )
            .unwrap();

        let total_supply: Amount = template_test.call_method(nft_component, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 5);

        let result = template_test
            .execute_and_commit_manifest(
                r#"
            let account = var!["account"];
            let sparkle_nft = var!["nft"];
            let nft_resx = var!["nft_resx"];

            let bucket = account.withdraw_non_fungible(nft_resx, NonFungibleId("Burn!"));
            sparkle_nft.burn(bucket);
            sparkle_nft.total_supply();
        "#,
                vars.clone(),
                // Needed for withdraw
                vec![account_owner],
            )
            .unwrap();

        assert_eq!(result.finalize.execution_results[3].decode::<Amount>().unwrap(), 4);

        let total_supply: Amount = template_test.call_method(nft_component, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 4);

        // Cannot mint it again
        template_test
            .execute_and_commit_manifest(
                r#"
            let account = var!["account"];
            let sparkle_nft = var!["nft"];
            let nft_resx = var!["nft_resx"];

            let nft_bucket = sparkle_nft.mint_specific(NonFungibleId("Burn!"));
            account.deposit(nft_bucket);
        "#,
                vars.clone(),
                vec![],
            )
            .unwrap_err();
    }

    #[test]
    fn get_non_fungibles_from_containers() {
        let (mut template_test, (account_address, account_owner), nft_component, nft_resx) = setup();

        let vars = vec![
            ("account", account_address.into()),
            ("nft", nft_component.into()),
            ("nft_resx", nft_resx.into()),
        ];

        let total_supply: Amount = template_test.call_method(nft_component, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 4);

        let result = template_test
            .execute_and_commit_manifest(
                r#"
            let sparkle_nft = var!["nft"];
            sparkle_nft.get_non_fungibles_from_bucket();
            sparkle_nft.get_non_fungibles_from_vault();
        "#,
                vars.clone(),
                vec![account_owner],
            )
            .unwrap();

        result.finalize.result.expect("execution failed");

        // sparkle_nft.get_non_fungibles_from_bucket()
        let nfts_from_bucket = result.finalize.execution_results[0]
            .decode::<Vec<NonFungible>>()
            .unwrap();
        assert_eq!(nfts_from_bucket.len(), 4);

        // sparkle_nft.get_non_fungibles_from_vault()
        let nfts_from_bucket = result.finalize.execution_results[1]
            .decode::<Vec<NonFungible>>()
            .unwrap();
        assert_eq!(nfts_from_bucket.len(), 4);
    }
}

mod emoji_id {
    use tari_template_lib::{constants::XTR, types::Amount};

    use super::*;

    #[derive(Debug, Clone, Serialize, Deserialize, Hash)]
    #[repr(i32)]
    pub enum Emoji {
        Smile = 0x00,
        Sweat = 0x01,
        Laugh = 0x02,
        Wink = 0x03,
    }

    #[derive(Debug, Clone, Serialize, Deserialize, Hash)]
    pub struct EmojiId(Vec<Emoji>);

    fn mint_emoji_id(
        test: &mut TemplateTest,
        account_address: ComponentAddress,
        faucet_resource: ResourceAddress,
        emoji_id_minter: ComponentAddress,
        emoji_id: &EmojiId,
        price: Amount,
        owner_proof: NonFungibleAddress,
    ) -> Result<FinalizeResult, String> {
        let transaction = Transaction::builder()
            .call_method(account_address, "withdraw", args![faucet_resource, price])
            .put_last_instruction_output_on_workspace("payment")
            .call_method(emoji_id_minter, "mint", args![emoji_id, Workspace("payment")])
            .put_last_instruction_output_on_workspace("emoji_id")
            .call_method(account_address, "deposit", args![Workspace("emoji_id")])
            .build_and_seal(test.secret_key());

        let result = test.execute_and_commit_on_success(transaction, vec![owner_proof]);
        if let Some(reason) = result.finalize.result.any_reject() {
            return Err(format!("Minting emoji id failed: {reason}"));
        }
        Ok(result.finalize)
    }

    #[test]
    #[allow(clippy::too_many_lines)]
    fn mint_emoji_ids() {
        let mut test = TemplateTest::new(vec!["tests/templates/nft/emoji_id"]);

        // create an account
        let (account_address, owner_proof, _) = test.create_funded_account();

        // initialize the emoji id minter
        let emoji_id_template = test.get_template_address("EmojiIdMinter");
        let max_emoji_id_len = 10_u64;
        let price = Amount::from(20);
        let result = test
            .build_and_execute(
                Transaction::builder().call_function(emoji_id_template, "new", args![XTR, max_emoji_id_len, price]),
                vec![owner_proof.clone()],
            )
            .unwrap_success();
        let emoji_id_minter: ComponentAddress = result.finalize.execution_results[0].decode().unwrap();
        let emoji_id_resource = result
            .finalize
            .result
            .expect("Emoji id initialization failed")
            .up_iter()
            .find_map(|(addr, _)| addr.as_resource_address())
            .unwrap();

        // At first, we don't have any emojis minted
        let total_supply: Amount = test.call_method(emoji_id_minter, "total_supply", args![], vec![]);
        assert_eq!(total_supply, 0);

        // mint a new emoji_id
        let emoji_id = EmojiId(vec![Emoji::Smile, Emoji::Laugh]);
        mint_emoji_id(
            &mut test,
            account_address,
            XTR,
            emoji_id_minter,
            &emoji_id,
            price,
            owner_proof.clone(),
        )
        .unwrap();

        // the supply of emoji ids should have increased
        let total_supply: Amount = test.call_method(emoji_id_minter, "total_supply", call_args![], vec![]);
        assert_eq!(total_supply, 1);
        // check that the account holds the newly minted nft
        let nft_balance: Amount = test.call_method(account_address, "balance", call_args![emoji_id_resource], vec![]);
        assert_eq!(nft_balance, 1);

        // emoji id are unique, so minting the same emojis again must fail
        mint_emoji_id(
            &mut test,
            account_address,
            XTR,
            emoji_id_minter,
            &emoji_id,
            price,
            owner_proof.clone(),
        )
        .unwrap_err();

        // emoji ids with invalid length must fail
        let too_long_emoji_id = iter::repeat_n(Emoji::Smile, max_emoji_id_len as usize + 1).collect();
        let emoji_id = EmojiId(too_long_emoji_id);
        mint_emoji_id(
            &mut test,
            account_address,
            XTR,
            emoji_id_minter,
            &emoji_id,
            price,
            owner_proof.clone(),
        )
        .unwrap_err();

        // mint another unique emoji id
        let emoji_id = EmojiId(vec![Emoji::Smile, Emoji::Wink]);
        mint_emoji_id(
            &mut test,
            account_address,
            XTR,
            emoji_id_minter,
            &emoji_id,
            price,
            owner_proof,
        )
        .unwrap();
    }
}

mod tickets {
    use super::*;

    #[test]
    #[allow(clippy::too_many_lines)]
    fn buy_and_redeem_ticket() {
        let mut test = TemplateTest::new(vec!["tests/templates/nft/tickets"]);

        // create an account
        let (account_address, owner_proof, secret) = test.create_funded_account();

        // initialize the ticket seller
        let ticket_template = test.get_template_address("TicketSeller");
        let initial_supply = 10;
        let price = Amount::from(20);
        let event_description = "My music festival".to_string();
        let result = test.execute_expect_success(
            Transaction::builder()
                .call_function(ticket_template, "new", args![initial_supply, price, event_description])
                .build_and_seal(&secret),
            vec![owner_proof.clone()],
        );
        let ticket_seller: ComponentAddress = result.finalize.execution_results[0].decode().unwrap();
        let ticket_resource = result
            .finalize
            .result
            .expect("TicketSeller initialization failed")
            .up_iter()
            .find_map(|(addr, _)| addr.as_resource_address())
            .unwrap();

        // at the beginning we have the initial supply of tickets
        let total_supply: Amount = test.call_method(ticket_seller, "total_supply", args![], vec![]);
        assert_eq!(total_supply, initial_supply);

        // buy a ticket
        test.execute_expect_success(
            Transaction::builder()
                .call_method(account_address, "withdraw", args![XTR, Amount(20)])
                .put_last_instruction_output_on_workspace("payment")
                .call_method(ticket_seller, "buy_ticket", args![Workspace("payment")])
                .put_last_instruction_output_on_workspace("nft_bucket")
                .call_method(account_address, "deposit", args![Workspace("nft_bucket")])
                .build_and_seal(&secret),
            vec![owner_proof],
        );

        // redeem a ticket
        let vaults = test
            .read_only_state_store()
            .get_vaults_for_account(account_address)
            .unwrap();
        let vault = vaults.get(&ticket_resource).unwrap();
        let ticket_ids = vault.get_non_fungible_ids();
        assert_eq!(ticket_ids.len(), 1);
        let ticket_id = ticket_ids.first().unwrap().clone();

        let vars = [
            ("account", account_address.into()),
            ("ticket_seller", ticket_seller.into()),
            // TODO: it's weird that the "redeem_ticket" method accepts a NonFungibleId, but we are passing a
            // SubstateId variable
            ("ticket_addr", ManifestValue::NonFungibleId(ticket_id.clone())),
        ];

        test.execute_and_commit_manifest(
            r#"
                let account = var!["account"];
                let ticket_seller = var!["ticket_seller"];
                let ticket_addr = var!["ticket_addr"];

                ticket_seller.redeem_ticket(ticket_addr);
            "#,
            vars.clone(),
            vec![],
        )
        .unwrap();

        #[derive(Debug, Clone, Serialize, Deserialize, Default)]
        pub struct Ticket {
            pub is_redeemed: bool,
        }

        let ticket_substate_addr = SubstateId::NonFungible(NonFungibleAddress::new(ticket_resource, ticket_id));
        let ticket_nft = test
            .read_only_state_store()
            .get_substate(&ticket_substate_addr)
            .unwrap()
            .into_substate_value()
            .into_non_fungible()
            .unwrap();

        assert!(
            ticket_nft
                .contents()
                .unwrap()
                .decode_mutable_data::<Ticket>()
                .unwrap()
                .is_redeemed
        );
    }
}

#[test]
fn test_builtin_templates() {
    let mut template_test = TemplateTest::new(vec!["tests/templates/builtin_templates"]);

    let account_template_address: TemplateAddress =
        template_test.call_function("BuiltinTest", "get_account_template_address", args![], vec![]);
    assert_eq!(account_template_address, ACCOUNT_TEMPLATE_ADDRESS);

    let account_nft_template_address: TemplateAddress =
        template_test.call_function("BuiltinTest", "get_account_nft_template_address", args![], vec![]);
    assert_eq!(account_nft_template_address, NFT_FAUCET_TEMPLATE_ADDRESS);
}
