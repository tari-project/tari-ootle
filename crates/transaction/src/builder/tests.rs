//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use tari_template_lib::types::TemplateAddress;

use crate::{
    args,
    args::{InstructionArg, WorkspaceOffsetId},
    builder::named_component_call::CallFromWorkspace,
    AllocatableAddressType,
    ComponentReference,
    Instruction,
    Transaction,
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
