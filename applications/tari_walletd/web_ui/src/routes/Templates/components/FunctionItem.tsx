import { Table, TableBody, TableCell, TableContainer, TableHead, TableRow, useTheme } from "@mui/material";
import { FunctionDef, Type as FuncType } from "@tari-project/ootle-ts-bindings";
import { SlCheck, SlClose } from "react-icons/sl";

interface FunctionItemProps {
  functionDef: FunctionDef;
}

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
  const argumentItems = args.map(({ name, arg_type }, i) => {
    if (name == "self") return;
    return (
      <TableRow key={`arg_${name}:${arg_type}`}>
        <TableCell size="small">{name}</TableCell>
        <TableCell size="small">{getTypeAsString(arg_type)}</TableCell>
      </TableRow>
    );
  });

  const isSelfArg = args.every((arg) => arg.name === "self");

  const argsTable =
    args?.length > 0 && !isSelfArg ? (
      <TableContainer style={{ borderRadius: theme.spacing(1), border: `1px solid ${theme.palette.divider}` }}>
        <Table
          size="small"
          padding="checkbox"
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
      <TableCell size="small">{name}</TableCell>
      <TableCell size="small">{is_mut ? <SlCheck size={25} /> : <SlClose size={25} />}</TableCell>
      <TableCell>{argsTable}</TableCell>
      <TableCell size="small">{getTypeAsString(output)}</TableCell>
    </TableRow>
  );
}
