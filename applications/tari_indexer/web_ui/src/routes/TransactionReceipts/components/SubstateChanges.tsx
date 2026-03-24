//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import { Stack, Table, TableBody, TableCell, TableContainer, TableHead, TableRow, Typography } from "@mui/material";
import type { UpSubstate } from "@tari-project/ootle-ts-bindings";
import { substateIdToString } from "@tari-project/ootle-ts-bindings";
import CopyToClipboard from "../../../Components/CopyToClipboard";
import { DataTableCell } from "../../../Components/StyledComponents";

export default function SubstateChanges({ upped }: { upped: UpSubstate[] }) {
  return (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <TableCell>Substate ID</TableCell>
            <TableCell>Version</TableCell>
          </TableRow>
        </TableHead>
        <TableBody>
          {upped.map((up, index) => {
            const idStr = substateIdToString(up.substate_id);
            return (
              <TableRow key={index}>
                <DataTableCell>
                  <Stack direction="row" alignItems="center">
                    <Typography variant="body2" sx={{ fontFamily: "monospace", wordBreak: "break-all" }}>
                      {idStr}
                    </Typography>
                    <CopyToClipboard copy={idStr} />
                  </Stack>
                </DataTableCell>
                <DataTableCell>{up.version}</DataTableCell>
              </TableRow>
            );
          })}
        </TableBody>
      </Table>
    </TableContainer>
  );
}
