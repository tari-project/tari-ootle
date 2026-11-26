//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import {
  Chip,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TableRow,
  Tooltip,
  useTheme,
} from "@mui/material";
import { NestedCell } from "@routes/Templates/components/StyledTableComponents";
import { FunctionDef } from "@tari-project/ootle-ts-bindings";
import { formatType } from "./formatType";

interface FunctionItemProps {
  functionDef: FunctionDef;
}

export default function FunctionItem({ functionDef }: FunctionItemProps) {
  const theme = useTheme();
  const { name, arguments: args, kind, output } = functionDef;
  const is_mut = typeof kind === "object" && "Method" in kind && kind.Method === "RefMut";
  const migrates_from = typeof kind === "object" && "Migration" in kind ? kind.Migration.from : null;

  const argumentItems = args.map(({ name, arg_type }) => {
    const formattedType = formatType(arg_type);
    return (
      <TableRow key={`arg_${name}:${formattedType}`}>
        <NestedCell>
          <code>{name}</code>
        </NestedCell>
        <NestedCell>
          <code>{formattedType}</code>
        </NestedCell>
      </TableRow>
    );
  });

  const argsTable =
    args?.length > 0 ? (
      <TableContainer style={{ borderRadius: theme.spacing(1), border: `1px solid ${theme.palette.divider}` }}>
        <Table
          size="small"
          style={{
            border: "none",
            background: theme.palette.accent.background,
          }}
        >
          <TableHead>
            <TableRow style={{ background: theme.palette.background.default }}>
              <TableCell>Name</TableCell>
              <TableCell>Type</TableCell>
            </TableRow>
          </TableHead>
          <TableBody>{argumentItems}</TableBody>
        </Table>
      </TableContainer>
    ) : (
      `-`
    );
  return (
    <TableRow>
      <NestedCell>
        <code>{name}</code>
        {is_mut && (
          <Tooltip title="This method mutates the component state">
            <Chip label="mut" size="small" color="warning" sx={{ ml: 1 }} />
          </Tooltip>
        )}
        {migrates_from && (
          <Tooltip title={`Migrates a component from ${formatType(migrates_from)}`}>
            <Chip label="migration" size="small" color="info" sx={{ ml: 1 }} />
          </Tooltip>
        )}
      </NestedCell>
      <NestedCell>{argsTable}</NestedCell>
      <NestedCell align="right">
        <code>{formatType(output)}</code>
      </NestedCell>
    </TableRow>
  );
}
