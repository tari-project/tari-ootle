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
import type { Instruction } from "@tari-project/ootle-ts-bindings";
import { BinaryTag } from "@utils/cbor";
import { toHexString } from "@utils/helpers";
import { decode } from "cbor2";
import { useState } from "react";

function RowData({ title, data }: { title: string; data: Instruction }, index: number) {
  const [open, setOpen] = useState(false);
  const theme = useTheme();
  return (
    <>
      <TableRow key={`${index}-1`}>
        <DataTableCell sx={{ borderTop: 1, borderTopColor: "divider", borderBottom: "none" }}>{title}</DataTableCell>
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
            <CodeBlockExpand title={title} content={inspectify(data)} />
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

  const addressMapper = (tag: BinaryTag, prefix: string): [BinaryTag, (value: any) => string] => [
    tag,
    (value: any) => prefix + "_" + toHexString(value.contents),
  ];

  const tags = new Map([
    addressMapper(BinaryTag.VaultId, "vault"),
    addressMapper(BinaryTag.ResourceAddress, "resource"),
    addressMapper(BinaryTag.ComponentAddress, "component"),
    addressMapper(BinaryTag.FeeClaim, "vnfp"),
    addressMapper(BinaryTag.TransactionReceipt, "txreceipt"),
  ]);

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
              return <RowData key={index} title={Object.keys(item)[0]} data={item} />;
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
