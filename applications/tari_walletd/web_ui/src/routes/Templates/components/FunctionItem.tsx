//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
import { Chip, Table, TableBody, TableCell, TableContainer, TableHead, TableRow, Tooltip, useTheme } from "@mui/material";
import { NestedCell } from "@routes/Templates/components/StyledTableComponents";
import { FunctionDef, Type as FuncType } from "@tari-project/ootle-ts-bindings";

interface FunctionItemProps {
  functionDef: FunctionDef;
}

// TODO - move to helpers
function getTypeAsString(funcType: FuncType): string {
  if (typeof funcType === "string") {
    return funcType;
  }

  const funcTypeKeys = Object.keys(funcType);
  if (funcTypeKeys.length > 0) {
    switch (funcTypeKeys[0]) {
      case "Vec": {
        return getTypeAsString(funcType["Vec" as keyof typeof funcType]);
      }
      case "Tuple": {
        return JSON.stringify(funcType["Tuple" as keyof typeof funcType]);
      }
      case "Other": {
        const other = funcType["Other" as keyof typeof funcType] as { name: string };
        return other.name;
      }
    }
  }

  return "Unknown";
}

export default function FunctionItem({ functionDef }: FunctionItemProps) {
  const theme = useTheme();
  const { name, arguments: args, is_mut, output } = functionDef;
  const is_migration = (functionDef as FunctionDef & { is_migration?: boolean }).is_migration;

  const argumentItems = args.map(({ name, arg_type }) => {
    if (name == "self") return;
    return (
      <TableRow key={`arg_${name}:${arg_type}`}>
        <NestedCell>
          <code>{name}</code>
        </NestedCell>
        <NestedCell>
          <code>{getTypeAsString(arg_type)}</code>
        </NestedCell>
      </TableRow>
    );
  });

  const isSelfArg = args.every((arg) => arg.name === "self");

  const argsTable =
    args?.length > 0 && !isSelfArg ? (
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
        {is_migration && (
          <Tooltip title="This is a migration function">
            <Chip label="migration" size="small" color="info" sx={{ ml: 1 }} />
          </Tooltip>
        )}
      </NestedCell>
      <NestedCell>{argsTable}</NestedCell>
      <NestedCell align="right">
        <code>{getTypeAsString(output)}</code>
      </NestedCell>
    </TableRow>
  );
}
