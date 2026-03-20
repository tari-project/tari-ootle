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

use std::{collections::HashMap, fs, str::FromStr};

use tari_bor::cbor;
use tari_engine_types::substate::SubstateId;
use tari_ootle_transaction::{
    AllocatableAddressType,
    ComponentReference,
    Instruction,
    args::WorkspaceOffsetId,
    call_args,
};
use tari_template_lib::types::{
    ComponentAddress,
    ObjectKey,
    TemplateAddress,
    constants::TARI_TOKEN,
    crypto::RistrettoPublicKeyBytes,
};
use tari_transaction_manifest::{ManifestInstructions, ManifestValue, parse_manifest};

#[test]
#[allow(clippy::too_many_lines)]
fn manifest_smoke_test() {
    let input = fs::read_to_string("tests/examples/picture_seller.rs").unwrap();
    let account_component = ComponentAddress::new([0u8; ObjectKey::LENGTH].into());
    let test_faucet_component = ComponentAddress::new([2u8; ObjectKey::LENGTH].into());
    let picture_seller_template =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    let globals = HashMap::from([
        ("account".to_string(), SubstateId::Component(account_component).into()),
        (
            "test_faucet".to_string(),
            SubstateId::Component(test_faucet_component).into(),
        ),
    ]);
    let ManifestInstructions {
        instructions,
        fee_instructions,
    } = parse_manifest(&input, globals, Default::default()).unwrap();

    let expected = vec![
        Instruction::CallFunction {
            address: picture_seller_template,
            function: "new".try_into().unwrap(),
            args: call_args![1_000u64],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 0 },
        Instruction::CallMethod {
            call: test_faucet_component.into(),
            method: "take_free_coins".try_into().unwrap(),
            args: call_args![1000],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 1 },
        Instruction::CallMethod {
            call: account_component.into(),
            method: "deposit".try_into().unwrap(),
            args: call_args![Workspace(1)],
        },
        Instruction::CallMethod {
            call: account_component.into(),
            method: "set_public_key".try_into().unwrap(),
            args: call_args![
                RistrettoPublicKeyBytes::from_hex("0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef")
                    .unwrap(),
                ComponentAddress::from_str(
                    "component_0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"
                )
                .unwrap(),
                cbor!({"some" => {"data" => [1, 2, 3]}}).unwrap()
            ],
        },
        Instruction::CallMethod {
            call: account_component.into(),
            method: "withdraw".try_into().unwrap(),
            args: call_args![TARI_TOKEN, 1_000],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 2 },
        Instruction::CallMethod {
            call: ComponentReference::Workspace(0),
            method: "buy".try_into().unwrap(),
            args: call_args![Workspace(2)],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 3 },
        Instruction::CallMethod {
            call: account_component.into(),
            method: "deposit".try_into().unwrap(),
            args: call_args![Workspace(3)],
        },
    ];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn workspace_component_reference() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            let comp = MyTemplate::new();
            let result = comp.do_something();
        }
    "#;

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    let ManifestInstructions {
        instructions,
        fee_instructions,
    } = parse_manifest(manifest, HashMap::new(), Default::default()).unwrap();

    let expected = vec![
        Instruction::CallFunction {
            address: template_addr,
            function: "new".try_into().unwrap(),
            args: call_args![],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 0 },
        Instruction::CallMethod {
            call: ComponentReference::Workspace(0),
            method: "do_something".try_into().unwrap(),
            args: call_args![],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 1 },
    ];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn allocate_address_macros() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            let component_addr = new_component_addr!();
            let resx_addr = new_resource_addr!();
            let comp = MyTemplate::with_address(component_addr, resx_addr);
        }
    "#;

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    let ManifestInstructions {
        instructions,
        fee_instructions,
    } = parse_manifest(manifest, HashMap::new(), Default::default()).unwrap();

    let expected = vec![
        Instruction::AllocateAddress {
            allocatable_type: AllocatableAddressType::Component,
            workspace_id: 0,
        },
        Instruction::AllocateAddress {
            allocatable_type: AllocatableAddressType::Resource,
            workspace_id: 1,
        },
        Instruction::CallFunction {
            address: template_addr,
            function: "with_address".try_into().unwrap(),
            args: call_args![Workspace(0), Workspace(1)],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 2 },
    ];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn local_function_inlining() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn setup() {
            let comp = MyTemplate::new();
            let result = comp.do_something();
        }

        fn main() {
            setup();
            MyTemplate::final_step();
        }
    "#;

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    let ManifestInstructions {
        instructions,
        fee_instructions,
    } = parse_manifest(manifest, HashMap::new(), Default::default()).unwrap();

    // setup() should be inlined: its instructions appear first, then final_step()
    let expected = vec![
        // From setup():
        Instruction::CallFunction {
            address: template_addr,
            function: "new".try_into().unwrap(),
            args: call_args![],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 0 },
        Instruction::CallMethod {
            call: ComponentReference::Workspace(0),
            method: "do_something".try_into().unwrap(),
            args: call_args![],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 1 },
        // From main() after setup():
        Instruction::CallFunction {
            address: template_addr,
            function: "final_step".try_into().unwrap(),
            args: call_args![],
        },
    ];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn recursive_function_exceeds_call_depth() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn recurse() {
            recurse();
        }

        fn main() {
            recurse();
        }
    "#;

    let result = parse_manifest(manifest, HashMap::new(), Default::default());
    let err = match result {
        Ok(_) => panic!("Expected MaxCallDepthExceeded error, but got Ok"),
        Err(e) => e,
    };
    assert!(
        err.to_string().contains("Maximum call depth"),
        "Expected MaxCallDepthExceeded error, got: {err}"
    );
}

#[test]
fn create_account_simple() {
    let manifest = r#"
        fn main() {
            let owner_pk = var!["owner_pk"];
            let account = create_account!(owner_pk);
        }
    "#;

    let pk_bytes = [42u8; 32];
    let globals = HashMap::from([(
        "owner_pk".to_string(),
        ManifestValue::Value(tari_bor::Value::Bytes(pk_bytes.to_vec())),
    )]);

    let ManifestInstructions {
        instructions,
        fee_instructions,
    } = parse_manifest(manifest, globals, Default::default()).unwrap();

    let expected = vec![
        Instruction::CreateAccount {
            owner_public_key: RistrettoPublicKeyBytes::from(pk_bytes),
            owner_rule: None,
            access_rules: None,
            bucket_workspace_id: None,
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 0 },
    ];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn create_account_with_bucket() {
    let manifest = r#"
        fn main() {
            let owner_pk = var!["owner_pk"];
            let source = var!["source"];
            let bucket = source.withdraw(TARI, 10);
            let account = create_account!(owner_pk, bucket = bucket);
        }
    "#;

    let pk_bytes = [42u8; 32];
    let source_component = ComponentAddress::new([1u8; ObjectKey::LENGTH].into());
    let globals = HashMap::from([
        (
            "owner_pk".to_string(),
            ManifestValue::Value(tari_bor::Value::Bytes(pk_bytes.to_vec())),
        ),
        ("source".to_string(), SubstateId::Component(source_component).into()),
    ]);

    let ManifestInstructions {
        instructions,
        fee_instructions,
    } = parse_manifest(manifest, globals, Default::default()).unwrap();

    let expected = vec![
        Instruction::CallMethod {
            call: source_component.into(),
            method: "withdraw".try_into().unwrap(),
            args: call_args![TARI_TOKEN, 10],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 0 },
        Instruction::CreateAccount {
            owner_public_key: RistrettoPublicKeyBytes::from(pk_bytes),
            owner_rule: None,
            access_rules: None,
            bucket_workspace_id: Some(WorkspaceOffsetId::new(0)),
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 1 },
    ];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn create_account_without_assignment() {
    let manifest = r#"
        fn main() {
            let owner_pk = var!["owner_pk"];
            create_account!(owner_pk);
        }
    "#;

    let pk_bytes = [42u8; 32];
    let globals = HashMap::from([(
        "owner_pk".to_string(),
        ManifestValue::Value(tari_bor::Value::Bytes(pk_bytes.to_vec())),
    )]);

    let ManifestInstructions {
        instructions,
        fee_instructions,
    } = parse_manifest(manifest, globals, Default::default()).unwrap();

    let expected = vec![Instruction::CreateAccount {
        owner_public_key: RistrettoPublicKeyBytes::from(pk_bytes),
        owner_rule: None,
        access_rules: None,
        bucket_workspace_id: None,
    }];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}
