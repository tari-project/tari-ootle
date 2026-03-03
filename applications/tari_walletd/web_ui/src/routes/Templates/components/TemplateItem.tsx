import { FluidTableCell } from "@components/StyledComponents";
import { Collapse, Table, TableBody, TableContainer, TableHead, TableRow } from "@mui/material";
import FunctionItem from "@routes/Templates/components/FunctionItem";
import { NestedCell } from "@routes/Templates/components/StyledTableComponents";
import { AuthoredTemplate } from "@tari-project/ootle-ts-bindings";

interface TemplateItemProps {
  template: AuthoredTemplate;
  isOpen?: boolean;
}

const COLUMNS = ["Name", "Mutable", "Arguments", "Output"];

export default function TemplateItem({ template, isOpen = false }: TemplateItemProps) {
  const { functions } = template;
  const headers = COLUMNS.map((c) => <NestedCell key={c}>{c}</NestedCell>);
  const items = functions.map((functionDef) => (
    <FunctionItem key={`function_${functionDef.name}`} functionDef={functionDef} />
  ));

  const functionTable = (
    <TableContainer>
      <Table>
        <TableHead>
          <TableRow>
            <NestedCell>Name</NestedCell>
            <NestedCell align="center">Mutable</NestedCell>
            <NestedCell>Arguments</NestedCell>
            <NestedCell align="right">Output Type</NestedCell>
          </TableRow>
        </TableHead>
        <TableBody>{items}</TableBody>
      </Table>
    </TableContainer>
  );

  return (
    <TableRow>
      <FluidTableCell colSpan={4} style={{ borderBottom: "none" }}>
        <Collapse in={isOpen} timeout="auto" unmountOnExit>
          <h3>Functions</h3>
          {functionTable}
        </Collapse>
      </FluidTableCell>
    </TableRow>
  );
}
