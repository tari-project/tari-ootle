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
use tari_template_lib_types::{
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
        ..
    } = parse_manifest(&input, globals, Default::default(), Default::default()).unwrap();

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
                cbor!({"some" => {"data" => [1, 2, 3]}})
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
        ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

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
        ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

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
        ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

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

    let result = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default());
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
        ..
    } = parse_manifest(manifest, globals, Default::default(), Default::default()).unwrap();

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
        ..
    } = parse_manifest(manifest, globals, Default::default(), Default::default()).unwrap();

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
        ..
    } = parse_manifest(manifest, globals, Default::default(), Default::default()).unwrap();

    let expected = vec![Instruction::CreateAccount {
        owner_public_key: RistrettoPublicKeyBytes::from(pk_bytes),
        owner_rule: None,
        access_rules: None,
        bucket_workspace_id: None,
    }];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn none_literal() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            MyTemplate::create("hello", None, 42);
        }
    "#;

    let ManifestInstructions {
        instructions,
        fee_instructions,
        ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    let expected = vec![Instruction::CallFunction {
        address: template_addr,
        function: "create".try_into().unwrap(),
        args: call_args!["hello", Literal(Option::<()>::None), 42],
    }];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn metadata_macro_empty() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            MyTemplate::create(metadata![]);
        }
    "#;

    let ManifestInstructions {
        instructions,
        fee_instructions,
        ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    use tari_template_lib_types::Metadata;
    let expected = vec![Instruction::CallFunction {
        address: template_addr,
        function: "create".try_into().unwrap(),
        args: call_args![Metadata::new()],
    }];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn metadata_macro_with_values() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            MyTemplate::create(metadata!["key=value"]);
        }
    "#;

    let ManifestInstructions {
        instructions,
        fee_instructions,
        ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    use tari_template_lib_types::Metadata;
    let expected_metadata: Metadata = "key=value".parse().unwrap();
    let expected = vec![Instruction::CallFunction {
        address: template_addr,
        function: "create".try_into().unwrap(),
        args: call_args![expected_metadata],
    }];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn empty_metadata_function_call_syntax() {
    // Metadata("") should also work now
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            MyTemplate::create(Metadata(""));
        }
    "#;

    let ManifestInstructions {
        instructions,
        fee_instructions,
        ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    use tari_template_lib_types::Metadata;
    let expected = vec![Instruction::CallFunction {
        address: template_addr,
        function: "create".try_into().unwrap(),
        args: call_args![Metadata::new()],
    }];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn tuple_destructuring() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            let comp = var!["comp"];
            let (a, b) = comp.redeem(100);
            comp.deposit(a);
            comp.deposit(b);
        }
    "#;

    let comp_address = ComponentAddress::new([5u8; ObjectKey::LENGTH].into());
    let globals = HashMap::from([("comp".to_string(), SubstateId::Component(comp_address).into())]);

    let ManifestInstructions {
        instructions,
        fee_instructions,
        ..
    } = parse_manifest(manifest, globals, Default::default(), Default::default()).unwrap();

    use tari_ootle_transaction::args::InstructionArg;

    let expected = vec![
        // comp.redeem(100) -> workspace key 0
        Instruction::CallMethod {
            call: comp_address.into(),
            method: "redeem".try_into().unwrap(),
            args: call_args![100],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 0 },
        // comp.deposit(a) where a = workspace 0, offset 0
        Instruction::CallMethod {
            call: comp_address.into(),
            method: "deposit".try_into().unwrap(),
            args: vec![InstructionArg::Workspace(WorkspaceOffsetId::new(0).with_offset(0))],
        },
        // comp.deposit(b) where b = workspace 0, offset 1
        Instruction::CallMethod {
            call: comp_address.into(),
            method: "deposit".try_into().unwrap(),
            args: vec![InstructionArg::Workspace(WorkspaceOffsetId::new(0).with_offset(1))],
        },
    ];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn tuple_destructuring_template_call() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            let (x, y, z) = MyTemplate::split(42);
        }
    "#;

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    let ManifestInstructions {
        instructions,
        fee_instructions,
        ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

    let expected = vec![
        Instruction::CallFunction {
            address: template_addr,
            function: "split".try_into().unwrap(),
            args: call_args![42],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 0 },
    ];

    assert_eq!(instructions, expected);
    assert_eq!(fee_instructions, vec![]);
}

/// `blob!(name)` should resolve to `InstructionArg::Blob(idx)` against the supplied blob map,
/// with the `Blobs` output ordered by first reference. Repeated references reuse the same
/// index.
#[test]
fn blob_macro_resolves_to_indexed_arg() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            let comp = MyTemplate::new(blob!(payload_a));
            comp.update(blob!("payload_b"), blob!(payload_a));
        }
    "#;

    let template_addr =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    let mut blob_inputs = HashMap::new();
    blob_inputs.insert(
        "payload_a".to_string(),
        tari_ootle_transaction::Blob::from(vec![1u8, 2, 3]),
    );
    blob_inputs.insert(
        "payload_b".to_string(),
        tari_ootle_transaction::Blob::from(vec![9u8, 8]),
    );

    let ManifestInstructions {
        instructions,
        fee_instructions,
        blobs,
    } = parse_manifest(manifest, HashMap::new(), Default::default(), blob_inputs).unwrap();

    // payload_a was referenced first so it gets index 0; payload_b is index 1.
    assert_eq!(blobs.len(), 2);
    assert_eq!(blobs.get(0).unwrap().as_bytes(), &[1u8, 2, 3]);
    assert_eq!(blobs.get(1).unwrap().as_bytes(), &[9u8, 8]);

    use tari_ootle_transaction::args::InstructionArg;
    assert_eq!(instructions[0], Instruction::CallFunction {
        address: template_addr,
        function: "new".try_into().unwrap(),
        args: vec![InstructionArg::Blob(0)],
    });
    // The second method call reuses payload_a — same index 0 — and adds payload_b at 1.
    assert_eq!(instructions[2], Instruction::CallMethod {
        call: ComponentReference::Workspace(0),
        method: "update".try_into().unwrap(),
        args: vec![InstructionArg::Blob(1), InstructionArg::Blob(0)],
    });
    assert_eq!(fee_instructions, vec![]);
}

#[test]
fn publish_template_macro_resolves_to_publish_template_instruction() {
    let manifest = r#"
        fn main() {
            publish_template!(wasm);
        }
    "#;

    let mut blob_inputs = HashMap::new();
    blob_inputs.insert(
        "wasm".to_string(),
        tari_ootle_transaction::Blob::from(vec![1u8, 2, 3, 4]),
    );

    let ManifestInstructions {
        instructions, blobs, ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), blob_inputs).unwrap();

    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs.get(0).unwrap().as_bytes(), &[1u8, 2, 3, 4]);
    assert_eq!(instructions.len(), 1);
    assert_eq!(instructions[0], Instruction::PublishTemplate {
        binary: 0,
        metadata_hash: None,
    });
}

#[test]
fn publish_template_resolves_blob_let_binding() {
    let manifest = r#"
        fn main() {
            let template = blob!("wasm");
            publish_template!(template);
        }
    "#;

    let mut blob_inputs = HashMap::new();
    blob_inputs.insert(
        "wasm".to_string(),
        tari_ootle_transaction::Blob::from(vec![1u8, 2, 3, 4]),
    );

    let ManifestInstructions {
        instructions, blobs, ..
    } = parse_manifest(manifest, HashMap::new(), Default::default(), blob_inputs).unwrap();

    assert_eq!(blobs.len(), 1);
    assert_eq!(blobs.get(0).unwrap().as_bytes(), &[1u8, 2, 3, 4]);
    assert_eq!(instructions.len(), 1);
    assert_eq!(instructions[0], Instruction::PublishTemplate {
        binary: 0,
        metadata_hash: None,
    });
}

#[test]
fn publish_template_macro_unknown_blob_errors() {
    let manifest = r#"
        fn main() {
            publish_template!(missing);
        }
    "#;
    let err = match parse_manifest(manifest, HashMap::new(), Default::default(), HashMap::new()) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("blob!('missing')"), "unexpected error: {err}");
}

#[test]
fn blob_macro_unknown_name_errors() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            let comp = MyTemplate::new(blob!(missing));
        }
    "#;

    let err = match parse_manifest(manifest, HashMap::new(), Default::default(), HashMap::new()) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("blob!('missing')"), "unexpected error: {err}");
}

#[test]
fn put_into_bucket_macro() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            let a = MyTemplate::make_bucket();
            let b = MyTemplate::make_bucket();
            put_into_bucket!(b, a);
        }
    "#;

    let ManifestInstructions { instructions, .. } =
        parse_manifest(manifest, HashMap::new(), Default::default(), Default::default()).unwrap();

    assert_eq!(instructions[4], Instruction::PutIntoBucket {
        src: WorkspaceOffsetId::new(1),
        dest: WorkspaceOffsetId::new(0),
    });
}

#[test]
fn put_into_bucket_unknown_variable_errors() {
    let manifest = r#"
        use template_c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7 as MyTemplate;

        fn main() {
            let a = MyTemplate::make_bucket();
            put_into_bucket!(missing, a);
        }
    "#;

    let err = match parse_manifest(manifest, HashMap::new(), Default::default(), HashMap::new()) {
        Ok(_) => panic!("expected an error"),
        Err(e) => e.to_string(),
    };
    assert!(err.contains("missing"), "unexpected error: {err}");
}
