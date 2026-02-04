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
use tari_ootle_transaction::{Instruction, call_args};
use tari_template_lib::types::{
    ComponentAddress,
    ObjectKey,
    TemplateAddress,
    constants::XTR,
    crypto::RistrettoPublicKeyBytes,
};
use tari_transaction_manifest::{ManifestInstructions, parse_manifest};

#[test]
#[allow(clippy::too_many_lines)]
fn manifest_smoke_test() {
    let input = fs::read_to_string("tests/examples/picture_seller.rs").unwrap();
    let account_component = ComponentAddress::new([0u8; ObjectKey::LENGTH].into());
    let picture_seller_component = ComponentAddress::new([1u8; ObjectKey::LENGTH].into());
    let test_faucet_component = ComponentAddress::new([2u8; ObjectKey::LENGTH].into());
    let picture_seller_template =
        TemplateAddress::from_hex("c2b621869ec2929d3b9503ea41054f01b468ce99e50254b58e460f608ae377f7").unwrap();

    let globals = HashMap::from([
        ("account".to_string(), SubstateId::Component(account_component).into()),
        (
            "picture_seller_addr".to_string(),
            SubstateId::Component(picture_seller_component).into(),
        ),
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
            args: call_args![XTR, 1_000],
        },
        Instruction::PutLastInstructionOutputOnWorkspace { key: 2 },
        Instruction::CallMethod {
            call: picture_seller_component.into(),
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
