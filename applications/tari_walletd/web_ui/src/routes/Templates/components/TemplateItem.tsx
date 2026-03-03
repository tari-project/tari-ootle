import { FluidTableCell } from "@components/StyledComponents";
import { Collapse, Table, TableBody, TableCell, TableContainer, TableHead, TableRow } from "@mui/material";
import { ArgDef, AuthoredTemplate, type FunctionDef, Type as FuncType } from "@tari-project/ootle-ts-bindings";
import { SlCheck, SlClose } from "react-icons/sl";

interface TemplateItemProps {
  template: AuthoredTemplate;
  isOpen?: boolean;
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

const COLUMNS = ["Name", "Mutable", "Arguments", "Output"];

export default function TemplateItem({ template, isOpen = false }: TemplateItemProps) {
  const headers = COLUMNS.map((c) => <TableCell key={c}>{c}</TableCell>);
  return (
    <TableRow>
      <FluidTableCell colSpan={4} style={{ borderBottom: "none" }}>
        <Collapse in={isOpen} timeout="auto" unmountOnExit>
          <h3>Functions</h3>
          {template.functions ? (
            <TableContainer>
              <Table>
                <TableHead>
                  <TableRow>{headers}</TableRow>
                </TableHead>
                <TableBody>
                  {template.functions.map((funcDef: FunctionDef, index: number) => {
                    return (
                      <TableRow key={index}>
                        <TableCell>{funcDef.name}</TableCell>
                        <TableCell>{funcDef.is_mut ? <SlCheck size={25} /> : <SlClose size={25} />}</TableCell>
                        <TableCell>
                          {funcDef.arguments.length > 0 ? (
                            <TableContainer>
                              <Table>
                                <TableHead>
                                  <TableRow>
                                    <TableCell>Name</TableCell>
                                    <TableCell>Type</TableCell>
                                  </TableRow>
                                </TableHead>
                                <TableBody>
                                  {funcDef.arguments.map((arg: ArgDef, index: number) => {
                                    return (
                                      <TableRow key={index}>
                                        <TableCell>{arg.name}</TableCell>
                                        <TableCell>{getTypeAsString(arg.arg_type)}</TableCell>
                                      </TableRow>
                                    );
                                  })}
                                </TableBody>
                              </Table>
                            </TableContainer>
                          ) : (
                            <SlClose size={25} color={"red"} />
                          )}
                        </TableCell>
                        <TableCell>{getTypeAsString(funcDef.output)}</TableCell>
                      </TableRow>
                    );
                  })}
                </TableBody>
              </Table>
            </TableContainer>
          ) : null}
        </Collapse>
      </FluidTableCell>
    </TableRow>
  );
}
