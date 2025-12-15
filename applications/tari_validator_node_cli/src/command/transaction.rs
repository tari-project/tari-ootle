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

use std::{
    fmt,
    path::{Path, PathBuf},
    str::FromStr,
    time::{Duration, Instant},
};

use anyhow::anyhow;
use clap::{Args, Subcommand};
use tari_engine::abi::Type;
use tari_engine_types::{
    commit_result::{ExecuteResult, FinalizeResult, RejectReason, TransactionResult},
    instruction_result::InstructionResult,
    parse_template_address,
    substate::{SubstateDiff, SubstateId, SubstateValue},
};
use tari_ootle_common_types::{
    displayable::{DisplayContainer, Displayable},
    optional::Optional,
    SubstateAddress,
    SubstateRequirement,
};
use tari_sidechain::QuorumDecision;
use tari_template_lib::{
    models::{BucketId, NonFungibleAddress, NonFungibleId},
    prelude::ResourceAddress,
    types::{Amount, TemplateAddress},
};
use tari_transaction::{
    arg,
    args::InstructionArg,
    builder::named_args::NamedArg,
    ComponentReference,
    Instruction,
    Transaction,
    TransactionId,
};
use tari_transaction_manifest::parse_manifest;
use tari_validator_node_client::{
    types::{
        DryRunTransactionFinalizeResult,
        GetTransactionResultRequest,
        GetTransactionResultResponse,
        SubmitTransactionRequest,
        SubmitTransactionResponse,
    },
    ValidatorNodeClient,
};
use tokio::time::MissedTickBehavior;

use crate::{command::manifest, component_manager::ComponentManager, from_hex::FromHex, key_manager::KeyManager};

#[derive(Debug, Subcommand, Clone)]
pub enum TransactionSubcommand {
    /// Retrieve transaction results by hash
    ///
    /// Queries the validator node for the execution result of a finalized transaction.
    /// Shows substates created/destroyed, return values, logs, and overall decision.
    ///
    /// Example:
    ///   tari_validator_node_cli transactions get 0xabc123...
    Get(GetArgs),

    /// Submit a single instruction transaction
    ///
    /// Creates and submits a transaction containing a single instruction (CallFunction or CallMethod).
    /// The transaction is automatically signed with the active key.
    /// Use --wait-for-result to block until the transaction is finalized.
    ///
    /// Example:
    ///   tari_validator_node_cli transactions submit call-function <template_address> <function> -a arg1 -a arg2 -w
    Submit(SubmitArgs),

    /// Submit a transaction from a manifest file
    ///
    /// Parses and executes a transaction manifest file containing multiple instructions.
    /// Manifests support variables that can be provided via the -g flag.
    /// The transaction is signed with the active key.
    ///
    /// Example:
    ///   tari_validator_node_cli transactions submit-manifest my_manifest.txt -g var1=value1 -w
    SubmitManifest(SubmitManifestArgs),
}

#[derive(Debug, Args, Clone)]
pub struct GetArgs {
    /// Transaction hash in hexadecimal format
    transaction_hash: FromHex<TransactionId>,
}

#[derive(Debug, Args, Clone)]
pub struct SubmitArgs {
    #[clap(subcommand)]
    pub instruction: CliInstruction,
    #[clap(flatten)]
    pub common: CommonSubmitArgs,
}

#[derive(Debug, Args, Clone)]
pub struct CommonSubmitArgs {
    /// Wait for the transaction to be finalized before returning
    ///
    /// When enabled, the CLI will poll the validator node until the transaction
    /// is finalized and display the complete execution result.
    #[clap(long, short = 'w')]
    pub wait_for_result: bool,

    /// Timeout in seconds when waiting for result (default: no timeout)
    ///
    /// Maximum time to wait for transaction finalization when --wait-for-result is enabled.
    /// The command will error if the timeout is exceeded.
    #[clap(long, short = 't')]
    pub wait_for_result_timeout: Option<u64>,

    /// Explicit input substates with versions
    ///
    /// Manually specify which substates the transaction will read.
    /// Format: <substate_id> or <substate_id>:<version>
    /// If not specified, inputs are automatically detected from CallMethod instructions.
    #[clap(long, short = 'i')]
    pub inputs: Vec<SubstateRequirement>,

    /// Transaction format version (for advanced use)
    #[clap(long, short = 'v')]
    pub version: Option<u8>,

    /// Component output tracking key (for advanced use)
    ///
    /// Used to track and store component outputs for future transactions.
    #[clap(long, short = 'd')]
    pub dump_outputs_into: Option<String>,

    /// Account template address override (for advanced use)
    #[clap(long, short = 'a')]
    pub account_template_address: Option<String>,
}

#[derive(Debug, Args, Clone)]
pub struct SubmitManifestArgs {
    /// Path to the manifest file
    ///
    /// A text file containing transaction instructions in manifest syntax.
    /// Use 'manifests new' to create a template.
    manifest: PathBuf,

    /// Global variables for the manifest in format name=value
    ///
    /// Manifests can reference variables that are substituted at runtime.
    /// Provide values using -g key=value format (can be specified multiple times).
    ///
    /// Example: -g component_addr=component_1234 -g amount=amount_100
    #[clap(long, short = 'g')]
    input_variables: Vec<String>,

    #[clap(flatten)]
    common: CommonSubmitArgs,
}

#[derive(Debug, Subcommand, Clone)]
pub enum CliInstruction {
    /// Call a template function
    ///
    /// Invokes a function on a template, typically to instantiate a new component.
    /// Template functions are stateless and don't operate on existing components.
    ///
    /// Arguments:
    ///   template_address - Hex address of the template
    ///   function_name - Name of the function to call
    ///   -a, --args - Function arguments (can be specified multiple times)
    ///
    /// Argument formats:
    ///   - Primitives: true, false, 123, -456
    ///   - Strings: hello_world
    ///   - Amounts: amount_1000
    ///   - NFT IDs: nft_u32:123
    ///   - Addresses: component_abc123, resource_def456
    ///
    /// Example:
    ///   call-function 0xabc123... new -a "MyToken" -a amount_1000000
    CallFunction {
        /// Template address in hexadecimal format
        template_address: FromHex<TemplateAddress>,
        /// Function name to invoke
        function_name: String,
        /// Function arguments (can be specified multiple times)
        #[clap(long, short = 'a')]
        args: Vec<CliArg>,
    },

    /// Call a method on a component
    ///
    /// Invokes a method on an existing component instance.
    /// The component state may be modified based on the method logic.
    ///
    /// Arguments:
    ///   component_address - Address of the component instance
    ///   method_name - Name of the method to call
    ///   -a, --args - Method arguments (can be specified multiple times)
    ///
    /// Example:
    ///   call-method component_abc123 transfer -a resource_def456 -a amount_100
    CallMethod {
        /// Component address (e.g., component_abc123...)
        component_address: SubstateId,
        /// Method name to invoke
        method_name: String,
        /// Method arguments (can be specified multiple times)
        #[clap(long, short = 'a')]
        args: Vec<CliArg>,
    },
}

impl TransactionSubcommand {
    pub async fn handle<P: AsRef<Path>>(
        self,
        base_dir: P,
        mut client: ValidatorNodeClient,
    ) -> Result<(), anyhow::Error> {
        match self {
            TransactionSubcommand::Submit(args) => {
                handle_submit(args, base_dir, &mut client).await?;
            },
            TransactionSubcommand::SubmitManifest(args) => {
                handle_submit_manifest(args, base_dir, &mut client).await?;
            },
            TransactionSubcommand::Get(args) => handle_get(args, &mut client).await?,
        }
        Ok(())
    }
}

async fn handle_get(args: GetArgs, client: &mut ValidatorNodeClient) -> Result<(), anyhow::Error> {
    let request = GetTransactionResultRequest {
        transaction_id: args.transaction_hash.into_inner(),
    };
    let Some(resp) = client.get_transaction_result(request).await.optional()? else {
        println!("Transaction not finalized");
        return Ok(());
    };

    let result = resp.transaction_execution.result();
    println!("Transaction {}", args.transaction_hash);
    println!();

    summarize_finalize_result(&result.finalize);

    Ok(())
}

pub async fn handle_submit(
    args: SubmitArgs,
    base_dir: impl AsRef<Path>,
    client: &mut ValidatorNodeClient,
) -> Result<SubmitTransactionResponse, anyhow::Error> {
    let SubmitArgs { instruction, common } = args;
    let instruction = match instruction {
        CliInstruction::CallFunction {
            template_address,
            function_name,
            args,
        } => Instruction::CallFunction {
            address: template_address.into_inner(),
            function: function_name.try_into()?,
            args: args
                .into_iter()
                .map(|s| {
                    s.into_named_arg()
                        .into_literal_bytes()
                        .map(InstructionArg::raw_literal_bytes)
                        .expect("CliArg does not support workspace args")
                })
                .collect(),
        },
        CliInstruction::CallMethod {
            component_address,
            method_name,
            args,
        } => Instruction::CallMethod {
            call: component_address
                .as_component_address()
                .ok_or_else(|| anyhow!("Invalid component address: {}", component_address))?
                .into(),
            method: method_name.try_into()?,
            args: args
                .into_iter()
                .map(|s| {
                    s.into_named_arg()
                        .into_literal_bytes()
                        .map(InstructionArg::raw_literal_bytes)
                        .expect("CliArg does not support workspace args")
                })
                .collect(),
        },
    };
    submit_transaction(vec![instruction], common, base_dir, client).await
}

async fn handle_submit_manifest(
    args: SubmitManifestArgs,
    base_dir: impl AsRef<Path>,
    client: &mut ValidatorNodeClient,
) -> Result<SubmitTransactionResponse, anyhow::Error> {
    let contents = std::fs::read_to_string(&args.manifest).map_err(|e| anyhow!("Failed to read manifest: {}", e))?;
    let instructions = parse_manifest(
        &contents,
        manifest::parse_globals(args.input_variables)?,
        Default::default(),
    )?;
    submit_transaction(instructions.instructions, args.common, base_dir, client).await
}

pub async fn submit_transaction(
    instructions: Vec<Instruction>,
    common: CommonSubmitArgs,
    base_dir: impl AsRef<Path>,
    client: &mut ValidatorNodeClient,
) -> Result<SubmitTransactionResponse, anyhow::Error> {
    let component_manager = ComponentManager::init(base_dir.as_ref())?;
    let key_manager = KeyManager::init(base_dir)?;
    let key = key_manager
        .get_active_key()
        .ok_or_else(|| anyhow::anyhow!("No active key. Use `keys use [public key hex]` to set one."))?;

    let inputs = if common.inputs.is_empty() {
        load_inputs(&instructions, &component_manager)?
    } else {
        common.inputs
    };

    // Convert to shard id
    let inputs = inputs.into_iter().collect::<Vec<_>>();

    summarize_request(&instructions, &inputs);
    println!();

    let transaction = Transaction::builder_localnet()
        .with_instructions(instructions)
        .with_inputs(inputs)
        .build_and_seal(&key.secret_key);

    let request = SubmitTransactionRequest { transaction };

    let mut resp = client.submit_transaction(request).await?;

    println!("✅ Transaction {} submitted.", resp.transaction_id);
    println!();

    let timer = Instant::now();
    if common.wait_for_result {
        println!("⏳️ Waiting for transaction result...");
        println!();
        let GetTransactionResultResponse {
            transaction_execution, ..
        } = wait_for_transaction_result(
            resp.transaction_id,
            client,
            common.wait_for_result_timeout.map(Duration::from_secs),
        )
        .await?;
        let result = transaction_execution.result();
        if transaction_execution.decision().is_commit() {
            if let Some(diff) = result.finalize.result.any_accept() {
                component_manager.commit_diff(diff)?;
            }
        }
        summarize(result, timer.elapsed());
        // Hack: submit response never returns a result unless it's a dry run - however cucumbers expect a result so add
        // it to the response here to satisfy that We'll remove these handlers eventually anyway
        resp.dry_run_result = Some(DryRunTransactionFinalizeResult {
            decision: if transaction_execution.decision().is_commit() {
                QuorumDecision::Accept
            } else {
                QuorumDecision::Reject
            },
            fee_breakdown: Some(result.finalize.fee_receipt.to_cost_breakdown()),
            finalize: result.finalize.clone(),
        });
    }

    Ok(resp)
}

async fn wait_for_transaction_result(
    transaction_id: TransactionId,
    client: &mut ValidatorNodeClient,
    timeout: Option<Duration>,
) -> anyhow::Result<GetTransactionResultResponse> {
    let mut interval = tokio::time::interval(Duration::from_secs(1));
    interval.set_missed_tick_behavior(MissedTickBehavior::Delay);
    let mut timeout = timeout;
    loop {
        let resp = client
            .get_transaction_result(GetTransactionResultRequest { transaction_id })
            .await
            .optional()?;

        if let Some(resp) = resp {
            return Ok(resp);
        }
        if let Some(t) = timeout {
            timeout = t.checked_sub(Duration::from_secs(1));
            if timeout.is_none() {
                return Err(anyhow!("Timeout waiting for transaction result"));
            }
        }
        interval.tick().await;
    }
}

fn summarize_request(instructions: &[Instruction], inputs: &[SubstateRequirement]) {
    println!("Inputs:");
    if inputs.is_empty() {
        println!("  None");
    } else {
        for substate_address in inputs {
            println!("- {}", substate_address);
        }
    }
    println!();
    println!("🌟 Submitting instructions:");
    for instruction in instructions {
        println!("- {}", instruction);
    }
    println!();
}

fn summarize(result: &ExecuteResult, time_taken: Duration) {
    println!("✅️ Transaction finalized",);
    println!();
    // println!("Epoch: {}", result.qc.epoch());
    // println!("Payload height: {}", result.qc.payload_height());
    // println!("Signed by: {} validator nodes", result.qc.validators_metadata().len());
    // println!();

    summarize_finalize_result(&result.finalize);

    println!();
    println!("Time taken: {:?}", time_taken);
    println!();
    println!("OVERALL DECISION: {}", result.finalize.result);
}

fn print_substate_diff(diff: &SubstateDiff) {
    for (address, substate) in diff.up_iter() {
        println!("️🌲 UP substate {} (v{})", address, substate.version(),);
        println!(
            "      🧩 Shard: {}",
            SubstateAddress::from_substate_id(address, substate.version())
        );
        match substate.substate_value() {
            SubstateValue::Component(component) => {
                println!("      ▶ component ({}): {}", component.module_name, address,);
            },
            SubstateValue::Resource(_) => {
                println!("      ▶ resource: {}", address);
            },
            SubstateValue::TransactionReceipt(_) => {
                println!("      ▶ transaction_receipt: {}", address);
            },
            SubstateValue::Vault(vault) => {
                println!("      ▶ vault: {} {}", address, vault.resource_address());
            },
            SubstateValue::NonFungible(_) => {
                println!("      ▶ NFT: {}", address);
            },
            SubstateValue::ClaimedOutputTombstone(_hash) => {
                println!("     ! layer one commitment: Should never happen");
            },
            SubstateValue::ValidatorFeePool(fee_pool) => {
                println!("      ▶ fee_pool: {}", address);
                println!("        ▶ amount: {}", fee_pool.amount());
                println!("        ▶ recipient: {}", fee_pool.claim_public_key());
            },
            SubstateValue::Template(_) => {
                println!("      ▶ Template: {}", address);
            },
            SubstateValue::Utxo(_) => {
                println!("      ▶ UTXO:");
                let utxo_addr = address.as_utxo_address().unwrap();
                let resx = utxo_addr.resource_address();
                let id = utxo_addr.id();
                println!("        ▶ Resource Address: {}", resx);
                println!("        ▶ ID: {}", id);
            },
        }
        println!();
    }
    for (address, version) in diff.down_iter() {
        println!("🗑️ DOWN substate {} v{}", address, version,);
        println!(
            "      🧩 Shard: {}",
            SubstateAddress::from_substate_id(address, *version)
        );
        println!();
    }
}

fn print_reject_reason(reason: &RejectReason) {
    println!("❌️ Transaction rejected: {}", reason);
}

#[allow(clippy::too_many_lines)]
fn summarize_finalize_result(finalize: &FinalizeResult) {
    println!("========= Substates =========");
    match finalize.result {
        TransactionResult::Accept(ref diff) => print_substate_diff(diff),
        TransactionResult::AcceptFeeRejectRest(ref diff, ref reason) => {
            print_substate_diff(diff);
            print_reject_reason(reason);
        },
        TransactionResult::Reject(ref reason) => print_reject_reason(reason),
    }

    println!("========= Return Values =========");
    for result in &finalize.execution_results {
        match &result.return_type {
            Type::Unit => {},
            Type::Bool => {
                println!("bool: {}", result.decode::<bool>().unwrap());
            },
            Type::I8 => {
                println!("i8: {}", result.decode::<i8>().unwrap());
            },
            Type::I16 => {
                println!("i16: {}", result.decode::<i16>().unwrap());
            },
            Type::I32 => {
                println!("i32: {}", result.decode::<i32>().unwrap());
            },
            Type::I64 => {
                println!("i64: {}", result.decode::<i64>().unwrap());
            },
            Type::I128 => {
                println!("i128: {}", result.decode::<i128>().unwrap());
            },
            Type::U8 => {
                println!("u8: {}", result.decode::<u8>().unwrap());
            },
            Type::U16 => {
                println!("u16: {}", result.decode::<u16>().unwrap());
            },
            Type::U32 => {
                println!("u32: {}", result.decode::<u32>().unwrap());
            },
            Type::U64 => {
                println!("u64: {}", result.decode::<u64>().unwrap());
            },
            Type::U128 => {
                println!("u128: {}", result.decode::<u128>().unwrap());
            },
            Type::String => {
                println!("string: {}", result.decode::<String>().unwrap());
            },
            Type::Vec(ty) => {
                let mut vec_ty = String::new();
                display_vec(&mut vec_ty, ty, result).unwrap();
                match &**ty {
                    Type::Other { name } => {
                        println!("Vec<{}>: {}", name, vec_ty);
                    },
                    _ => {
                        println!("Vec<{:?}>: {}", ty, vec_ty);
                    },
                }
            },
            Type::Tuple(subtypes) => {
                let str = format_tuple(subtypes, result);
                println!("{}", str);
            },
            Type::Other { ref name } if name == "Amount" => {
                println!("{}: {}", name, result.decode::<Amount>().unwrap());
            },
            Type::Other { ref name } if name == "Bucket" => {
                println!("{}: {}", name, result.decode::<BucketId>().unwrap());
            },
            Type::Other { ref name } => {
                println!("{}: {}", name, serde_json::to_string(&result.indexed).unwrap());
            },
        }
    }

    println!();
    println!("========= LOGS =========");
    for log in &finalize.logs {
        println!("{}", log);
    }
}

fn display_vec<W: fmt::Write>(writer: &mut W, ty: &Type, result: &InstructionResult) -> fmt::Result {
    fn display_slice<T: fmt::Display>(slice: &[T]) -> DisplayContainer<&[T]> {
        slice.display()
    }

    match &ty {
        Type::Unit => {},
        Type::Bool => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<bool>>().unwrap()))?;
        },
        Type::I8 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<i8>>().unwrap()))?;
        },
        Type::I16 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<i16>>().unwrap()))?;
        },
        Type::I32 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<i32>>().unwrap()))?;
        },
        Type::I64 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<i64>>().unwrap()))?;
        },
        Type::I128 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<i128>>().unwrap()))?;
        },
        Type::U8 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<u8>>().unwrap()))?;
        },
        Type::U16 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<u16>>().unwrap()))?;
        },
        Type::U32 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<u32>>().unwrap()))?;
        },
        Type::U64 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<u64>>().unwrap()))?;
        },
        Type::U128 => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<u128>>().unwrap()))?;
        },
        Type::String => {
            write!(writer, "{}", result.decode::<Vec<String>>().unwrap().join(", "))?;
        },
        Type::Vec(ty) => {
            let mut vec_ty = String::new();
            display_vec(&mut vec_ty, ty, result)?;
            match &**ty {
                Type::Other { name } => {
                    write!(writer, "Vec<{}>: {}", name, vec_ty)?;
                },
                _ => {
                    write!(writer, "Vec<{:?}>: {}", ty, vec_ty)?;
                },
            }
        },
        Type::Tuple(subtypes) => {
            let str = format_tuple(subtypes, result);
            write!(writer, "{}", str)?;
        },
        Type::Other { name } if name == "Amount" => {
            write!(writer, "{}", display_slice(&result.decode::<Vec<Amount>>().unwrap()))?;
        },
        Type::Other { name } if name == "NonFungibleId" => {
            write!(
                writer,
                "{}",
                display_slice(&result.decode::<Vec<NonFungibleId>>().unwrap())
            )?;
        },
        Type::Other { .. } => {
            write!(writer, "{}", serde_json::to_string(&result.indexed).unwrap())?;
        },
    }
    Ok(())
}

fn format_tuple(subtypes: &[Type], result: &InstructionResult) -> String {
    let tuple_type = Type::Tuple(subtypes.to_vec());
    let result_json = serde_json::to_string(&result.indexed).unwrap();
    format!("{}: {}", tuple_type, result_json)
}

fn load_inputs(
    instructions: &[Instruction],
    component_manager: &ComponentManager,
) -> Result<Vec<SubstateRequirement>, anyhow::Error> {
    let mut inputs = Vec::new();
    for instruction in instructions {
        if let Instruction::CallMethod {
            call: ComponentReference::Address(component_address),
            ..
        } = instruction
        {
            let addr = SubstateId::Component(*component_address);
            if inputs.iter().any(|a: &SubstateRequirement| a.substate_id == addr) {
                continue;
            }
            let component = component_manager
                .get_root_substate(&addr)?
                .ok_or_else(|| anyhow!("Component {} not found", component_address))?;
            println!("Loaded inputs");
            println!("- {} v{}", addr, component.latest_version());
            inputs.push(SubstateRequirement {
                substate_id: addr,
                version: Some(component.latest_version()),
            });
            for child in component.get_children() {
                println!("  - {} v{:?}", child.substate_id, child.version);
            }
            inputs.extend(component.get_children());
        }
    }
    Ok(inputs)
}

#[derive(Debug, Clone)]
pub enum CliArg {
    String(String),
    U64(u64),
    U32(u32),
    U16(u16),
    U8(u8),
    I64(i64),
    I32(i32),
    I16(i16),
    I8(i8),
    Bool(bool),
    Amount(i64),
    NonFungibleId(NonFungibleId),
    SubstateId(SubstateId),
    TemplateAddress(TemplateAddress),
}

impl FromStr for CliArg {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Ok(v) = s.parse::<u64>() {
            return Ok(CliArg::U64(v));
        }
        if let Ok(v) = s.parse::<u32>() {
            return Ok(CliArg::U32(v));
        }
        if let Ok(v) = s.parse::<u16>() {
            return Ok(CliArg::U16(v));
        }
        if let Ok(v) = s.parse::<u8>() {
            return Ok(CliArg::U8(v));
        }
        if let Ok(v) = s.parse::<i64>() {
            return Ok(CliArg::I64(v));
        }
        if let Ok(v) = s.parse::<i32>() {
            return Ok(CliArg::I32(v));
        }
        if let Ok(v) = s.parse::<i16>() {
            return Ok(CliArg::I16(v));
        }
        if let Ok(v) = s.parse::<i8>() {
            return Ok(CliArg::I8(v));
        }
        if let Ok(v) = s.parse::<bool>() {
            return Ok(CliArg::Bool(v));
        }

        if let Ok(v) = s.parse::<SubstateId>() {
            return Ok(CliArg::SubstateId(v));
        }

        if let Some(v) = parse_template_address(s) {
            return Ok(CliArg::TemplateAddress(v));
        }

        if let Some(("nft", nft_id)) = s.split_once('_') {
            match NonFungibleId::try_from_canonical_string(nft_id) {
                Ok(v) => {
                    return Ok(CliArg::NonFungibleId(v));
                },
                Err(e) => {
                    eprintln!(
                        "WARN: '{}' is not a valid NonFungibleId ({:?}) and will be interpreted as a string",
                        s, e
                    );
                },
            }
        }

        if let Some(("amount", amount)) = s.split_once('_') {
            match amount.parse::<i64>() {
                Ok(number) => {
                    return Ok(CliArg::Amount(number));
                },
                Err(e) => {
                    eprintln!(
                        "WARN: '{}' is not a valid Amount ({:?}) and will be interpreted as a string",
                        s, e
                    );
                },
            }
        }

        Ok(CliArg::String(s.to_string()))
    }
}

impl CliArg {
    pub fn into_named_arg(self) -> NamedArg {
        match self {
            CliArg::String(s) => arg!(s),
            CliArg::U64(v) => arg!(v),
            CliArg::U32(v) => arg!(v),
            CliArg::U16(v) => arg!(v),
            CliArg::U8(v) => arg!(v),
            CliArg::I64(v) => arg!(v),
            CliArg::I32(v) => arg!(v),
            CliArg::I16(v) => arg!(v),
            CliArg::I8(v) => arg!(v),
            CliArg::Bool(v) => arg!(v),
            CliArg::SubstateId(v) => match v {
                SubstateId::Component(v) => arg!(v),
                SubstateId::Resource(v) => arg!(v),
                SubstateId::Vault(v) => arg!(v),
                SubstateId::ClaimedOutputTombstone(v) => arg!(v),
                SubstateId::NonFungible(v) => arg!(v),
                SubstateId::TransactionReceipt(v) => arg!(v),
                SubstateId::Template(v) => arg!(v),
                SubstateId::ValidatorFeePool(v) => arg!(v),
                SubstateId::Utxo(v) => arg!(v),
            },
            CliArg::TemplateAddress(v) => arg!(v),
            CliArg::NonFungibleId(v) => arg!(v),
            CliArg::Amount(v) => arg!(Amount(v)),
        }
    }
}

#[derive(Debug, Clone)]
pub struct NewResourceOutput {
    pub template_address: TemplateAddress,
    pub token_symbol: String,
}

impl FromStr for NewResourceOutput {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (template_address, token_symbol) = s
            .split_once(':')
            .ok_or_else(|| anyhow!("Expected template address and token symbol"))?;
        let template_address = TemplateAddress::from_hex(template_address)?;
        Ok(NewResourceOutput {
            template_address,
            token_symbol: token_symbol.to_string(),
        })
    }
}

#[derive(Debug, Clone)]
pub struct SpecificNonFungibleMintOutput {
    pub resource_address: ResourceAddress,
    pub non_fungible_id: NonFungibleId,
}

impl SpecificNonFungibleMintOutput {
    pub fn to_substate_address(&self) -> SubstateId {
        SubstateId::NonFungible(NonFungibleAddress::new(
            self.resource_address,
            self.non_fungible_id.clone(),
        ))
    }
}

impl FromStr for SpecificNonFungibleMintOutput {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (resource_address, non_fungible_id) = s
            .split_once(',')
            .ok_or_else(|| anyhow!("Expected resource address and non-fungible id"))?;
        let resource_address = SubstateId::from_str(resource_address)?;
        let resource_address = resource_address
            .as_resource_address()
            .ok_or_else(|| anyhow!("Expected resource address but got {}", resource_address))?;
        let non_fungible_id = non_fungible_id
            .split_once('_')
            .map(|(_, b)| b)
            .unwrap_or(non_fungible_id);
        let non_fungible_id =
            NonFungibleId::try_from_canonical_string(non_fungible_id).map_err(|e| anyhow!("{:?}", e))?;
        Ok(SpecificNonFungibleMintOutput {
            resource_address,
            non_fungible_id,
        })
    }
}

#[derive(Debug, Clone)]
pub struct NewNonFungibleMintOutput {
    pub resource_address: ResourceAddress,
    pub count: u8,
}

impl FromStr for NewNonFungibleMintOutput {
    type Err = anyhow::Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (resource_address, count_str) = s.split_once(',').unwrap_or((s, "1"));
        let resource_address = SubstateId::from_str(resource_address)?;
        let resource_address = resource_address
            .as_resource_address()
            .ok_or_else(|| anyhow!("Expected resource address but got {}", resource_address))?;
        Ok(NewNonFungibleMintOutput {
            resource_address,
            count: count_str.parse()?,
        })
    }
}
