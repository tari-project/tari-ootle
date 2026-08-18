//  Copyright 2022. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import CodeBlockExpand from "@components/CodeBlock";
import { AccordionIconButton, DataTableCell } from "@components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import { Box, Collapse, Table, TableBody, TableContainer, TableRow, Typography } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import type { Instruction, InstructionArg, WorkspaceOffsetId } from "@tari-project/ootle-ts-bindings";
import { TariTypeTag } from "@tari-project/ootle-ts-bindings";
import { toHexString } from "@utils/helpers";
import { decode } from "cbor2";
import { useState } from "react";

const literalDecodeTags = () => {
  const addressMapper = (tag: TariTypeTag, prefix: string): [TariTypeTag, (value: any) => string] => [
    tag,
    (value: any) => prefix + "_" + toHexString(value.contents),
  ];
  return new Map([
    addressMapper(TariTypeTag.VaultId, "vault"),
    addressMapper(TariTypeTag.ResourceAddress, "resource"),
    addressMapper(TariTypeTag.ComponentAddress, "component"),
    addressMapper(TariTypeTag.ValidatorNodeFeePool, "vnfp"),
    addressMapper(TariTypeTag.TransactionReceipt, "txreceipt"),
  ]);
};

/// Compact display of an arbitrary decoded CBOR value. JSON.stringify is not
/// usable here: cbor2 decodes large integers to BigInt, which it rejects.
function compactValue(value: unknown): string {
  if (value === null || value === undefined) {
    return "null";
  }
  if (typeof value === "string") {
    return JSON.stringify(value);
  }
  if (typeof value === "number" || typeof value === "bigint" || typeof value === "boolean") {
    return String(value);
  }
  if (value instanceof Uint8Array) {
    return `0x${toHexString(Array.from(value))}`;
  }
  if (Array.isArray(value)) {
    return `[${value.map(compactValue).join(", ")}]`;
  }
  if (value instanceof Map) {
    return `{${[...value.entries()].map(([k, v]) => `${compactValue(k)}: ${compactValue(v)}`).join(", ")}}`;
  }
  return `{${Object.entries(value)
    .map(([k, v]) => `${k}: ${compactValue(v)}`)
    .join(", ")}}`;
}

function workspaceId(id: WorkspaceOffsetId): string {
  return id.offset != null ? `$${id.id}.${id.offset}` : `$${id.id}`;
}

function formatArg(arg: InstructionArg): string {
  if (typeof arg === "object" && "Literal" in arg) {
    try {
      return `Lit(${compactValue(decode(arg.Literal, { encoding: "hex", tags: literalDecodeTags() }))})`;
    } catch {
      return `Lit(0x${arg.Literal})`;
    }
  }
  if (typeof arg === "object" && "Workspace" in arg) {
    return `Workspace(${workspaceId(arg.Workspace)})`;
  }
  if (typeof arg === "object" && "Blob" in arg) {
    return `Blob(#${arg.Blob})`;
  }
  return compactValue(arg);
}

/// One-line call-style rendering of an instruction, e.g.
/// `Call(component_21f1…, "pay_fee", Lit([956, 0]))`. The expandable JSON
/// below the row remains the full-fidelity view.
function summarize(instruction: Instruction): string {
  if (typeof instruction === "string") {
    return instruction;
  }
  if ("CreateAccount" in instruction) {
    const { owner_public_key, bucket_workspace_id } = instruction.CreateAccount;
    const bucket = bucket_workspace_id != null ? `, bucket: ${workspaceId(bucket_workspace_id)}` : "";
    return `CreateAccount("${owner_public_key}"${bucket})`;
  }
  if ("CallFunction" in instruction) {
    const { address, function: fn, args } = instruction.CallFunction;
    return `Call(template_${address}, "${fn}"${args.length ? ", " + args.map(formatArg).join(", ") : ""})`;
  }
  if ("CallMethod" in instruction) {
    const { call, method, args } = instruction.CallMethod;
    const target = "Address" in call ? call.Address : `$${call.Workspace}`;
    return `Call(${target}, "${method}"${args.length ? ", " + args.map(formatArg).join(", ") : ""})`;
  }
  if ("PutLastInstructionOutputOnWorkspace" in instruction) {
    return `PutLastInstructionOutputOnWorkspace($${instruction.PutLastInstructionOutputOnWorkspace.key})`;
  }
  if ("ClaimBurn" in instruction) {
    return `ClaimBurn(${instruction.ClaimBurn.claim.commitment})`;
  }
  if ("ClaimValidatorFees" in instruction) {
    return `ClaimValidatorFees(${instruction.ClaimValidatorFees.address})`;
  }
  if ("Assert" in instruction) {
    const { key, assertion } = instruction.Assert;
    const kind = typeof assertion === "string" ? assertion : Object.keys(assertion)[0];
    return `Assert(${workspaceId(key)}, ${kind})`;
  }
  if ("TakeFromBucket" in instruction) {
    const { input_bucket, amount, output_bucket } = instruction.TakeFromBucket;
    return `TakeFromBucket(${workspaceId(input_bucket)}, ${amount} -> $${output_bucket})`;
  }
  if ("PublishTemplate" in instruction) {
    return `PublishTemplate(Blob(#${instruction.PublishTemplate.binary}))`;
  }
  if ("AllocateAddress" in instruction) {
    const { allocatable_type, workspace_id } = instruction.AllocateAddress;
    return `AllocateAddress(${allocatable_type} -> $${workspace_id})`;
  }
  if ("StealthTransfer" in instruction) {
    const { resource_address_ref, statement, revealed_input_bucket } = instruction.StealthTransfer;
    const resource =
      "Address" in resource_address_ref ? resource_address_ref.Address : workspaceId(resource_address_ref.Workspace);
    const inputBucket = revealed_input_bucket != null ? `, input bucket: ${workspaceId(revealed_input_bucket)}` : "";
    const counts = `${statement.inputs_statement?.inputs?.length ?? 0} in / ${statement.outputs_statement?.outputs?.length ?? 0} out`;
    return `StealthTransfer(${resource}, ${counts}${inputBucket})`;
  }
  if ("PayFeeFromBucket" in instruction) {
    return `PayFeeFromBucket(${workspaceId(instruction.PayFeeFromBucket.bucket)})`;
  }
  if ("UpdateComponentTemplate" in instruction) {
    const { component, new_template } = instruction.UpdateComponentTemplate;
    const target = "Address" in component ? component.Address : `$${component.Workspace}`;
    return `UpdateComponentTemplate(${target} -> template_${new_template})`;
  }
  if ("PutIntoBucket" in instruction) {
    const { src, dest } = instruction.PutIntoBucket;
    return `PutIntoBucket(${workspaceId(src)} -> ${workspaceId(dest)})`;
  }
  return Object.keys(instruction)[0];
}

function instructionName(instruction: Instruction): string {
  return typeof instruction === "string" ? instruction : Object.keys(instruction)[0];
}

interface RowDataProps {
  title: string;
  data: Instruction;
  index: number;
}
function RowData({ title, data, index }: RowDataProps) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();
  return (
    <>
      <TableRow key={`${index}-1`}>
        <DataTableCell
          sx={{
            borderTop: 1,
            borderTopColor: "divider",
            borderBottom: "none",
            fontFamily: "monospace",
            fontSize: "0.8rem",
            wordBreak: "break-all",
          }}
        >
          {title}
        </DataTableCell>
        <DataTableCell
          width={90}
          sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none", textAlign: "center" }}
        >
          <AccordionIconButton
            aria-label="expand row"
            size="small"
            onClick={() => {
              setOpen(!open);
            }}
          >
            {open ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
          </AccordionIconButton>
        </DataTableCell>
      </TableRow>
      <TableRow key={`${index}-2`}>
        <DataTableCell
          style={{
            paddingBottom: theme.spacing(1),
            paddingTop: 0,
            borderBottom: "none",
          }}
          colSpan={2}
        >
          <Collapse in={open} timeout="auto" unmountOnExit>
            <CodeBlockExpand title={instructionName(data)} content={inspectify(data)} />
          </Collapse>
        </DataTableCell>
      </TableRow>
    </>
  );
}

function inspectify(instruction: Instruction) {
  let method;
  if (typeof instruction !== "object" || instruction === null) {
    return instruction;
  }

  if ("CallFunction" in instruction) {
    method = "CallFunction" as keyof Instruction;
  } else if ("CallMethod" in instruction) {
    method = "CallMethod" as keyof Instruction;
  } else {
    return instruction;
  }

  const tags = literalDecodeTags();
  const contents = instruction[method] as any;
  const args = contents.args.map((arg: { Literal: string }) => {
    if ("Literal" in arg) {
      return { Literal: decode(arg.Literal, { encoding: "hex", tags }) };
    }
    return arg;
  });
  return {
    [method]: {
      ...contents,
      args,
    },
  };
}

export default function Instructions({ data }: { data: Array<Instruction> }) {
  return (
    <TableContainer>
      <Table>
        <TableBody>
          {data?.length ? (
            data.map((item: Instruction, index) => {
              return <RowData key={index} index={index} title={summarize(item)} data={item} />;
            })
          ) : (
            <Box sx={{ p: 3, textAlign: "center" }}>
              <Typography variant="body2" color="text.secondary">
                No instructions available
              </Typography>
            </Box>
          )}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
