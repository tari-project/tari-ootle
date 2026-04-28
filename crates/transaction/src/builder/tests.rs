//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib_types::TemplateAddress;

use crate::{
    AllocatableAddressType,
    ComponentReference,
    Instruction,
    Transaction,
    args,
    args::{InstructionArg, WorkspaceOffsetId},
    builder::named_component_call::CallFromWorkspace,
};

#[test]
fn it_converts_workspace_names_to_ids() {
    let transaction = Transaction::builder_localnet()
        .put_last_instruction_output_on_workspace("thing1")
        .allocate_resource_address("thing2")
        .allocate_component_address("thing3")
        .call_function(TemplateAddress::default(), "do_something", args![
            Workspace("thing1"),
            "thing2",
            Workspace("thing1.0"),
            Workspace("thing2.2")
        ])
        .call_method(CallFromWorkspace::new("thing3"), "do_something_else", args![Workspace(
            "thing1"
        )])
        .build_unsigned();

    assert_eq!(
        transaction.instructions()[0],
        Instruction::PutLastInstructionOutputOnWorkspace { key: 0 }
    );
    assert_eq!(transaction.instructions()[1], Instruction::AllocateAddress {
        allocatable_type: AllocatableAddressType::Resource,
        workspace_id: 1,
    });
    assert_eq!(transaction.instructions()[2], Instruction::AllocateAddress {
        allocatable_type: AllocatableAddressType::Component,
        workspace_id: 2,
    });
    assert_eq!(transaction.instructions()[3], Instruction::CallFunction {
        address: TemplateAddress::default(),
        function: "do_something".try_into().unwrap(),
        args: vec![
            InstructionArg::Workspace(WorkspaceOffsetId::new(0)),
            InstructionArg::from_type(&"thing2").unwrap(),
            InstructionArg::Workspace(WorkspaceOffsetId::new(0).with_offset(0)),
            InstructionArg::Workspace(WorkspaceOffsetId::new(1).with_offset(2))
        ]
    });
    assert_eq!(transaction.instructions()[4], Instruction::CallMethod {
        call: ComponentReference::Workspace(2),
        method: "do_something_else".try_into().unwrap(),
        args: vec![InstructionArg::Workspace(WorkspaceOffsetId::new(0))]
    });
}

/// Merge must remap blob indices and append blobs from `other` so references stay valid.
#[test]
fn merge_remaps_blob_ids_and_appends_blobs() {
    let address = TemplateAddress::from_array([7; 32]);

    // First builder owns one blob `a` referenced by an arg.
    let a = Transaction::builder_localnet()
        .add_blob("a", vec![1u8, 2, 3])
        .call_function(address, "f", args![Blob("a")]);

    // Second builder owns its own blob `b`.
    let b = Transaction::builder_localnet()
        .add_blob("b", vec![4u8, 5])
        .call_function(address, "g", args![Blob("b")]);

    let merged = a.merge(b).build_unsigned();

    // Both blobs are present, in order.
    let blobs = merged.blobs();
    assert_eq!(blobs.len(), 2);
    assert_eq!(blobs.get(0).unwrap().as_bytes(), &[1u8, 2, 3]);
    assert_eq!(blobs.get(1).unwrap().as_bytes(), &[4u8, 5]);

    // The first instruction's Blob arg still references index 0 (no shift, it was already
    // on `self`); the second's Blob arg has been shifted from 0 → 1 during merge.
    assert_eq!(merged.instructions()[0], Instruction::CallFunction {
        address,
        function: "f".try_into().unwrap(),
        args: vec![InstructionArg::Blob(0)],
    });
    assert_eq!(merged.instructions()[1], Instruction::CallFunction {
        address,
        function: "g".try_into().unwrap(),
        args: vec![InstructionArg::Blob(1)],
    });
}

#[test]
#[should_panic(expected = "blob name 'a' collides during merge")]
fn merge_rejects_colliding_blob_names() {
    let a = Transaction::builder_localnet().add_blob("a", vec![1u8]);
    let b = Transaction::builder_localnet().add_blob("a", vec![2u8]);
    let _ = a.merge(b);
}

#[test]
fn merge_remaps_publish_template_blob_index() {
    // `self` already has a blob, so the merged builder's auto-added template blob index 0
    // becomes index 1 after merge.
    let a = Transaction::builder_localnet().add_blob("filler", vec![0u8; 4]);
    let b = Transaction::builder_localnet().publish_template(vec![9u8, 9, 9]);

    let merged = a.merge(b).build_unsigned();

    let blobs = merged.blobs();
    assert_eq!(blobs.len(), 2);
    assert_eq!(blobs.get(0).unwrap().as_bytes(), &[0u8; 4][..]);
    assert_eq!(blobs.get(1).unwrap().as_bytes(), &[9u8, 9, 9]);

    assert_eq!(merged.instructions()[0], Instruction::PublishTemplate {
        binary: 1,
        metadata_hash: None,
    });
}
