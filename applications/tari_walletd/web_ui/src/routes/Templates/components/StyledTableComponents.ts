import TableCell, { TableCellProps } from "@mui/material/TableCell";
import { styled } from "@mui/material/styles";

export const NestedCell: React.FC<TableCellProps> = styled(TableCell)<TableCellProps>`
  vertical-align: top;
  code {
    border: 1px solid ${({ theme }) => theme.palette.divider};
    background: ${({ theme }) => theme.palette.accent.background};
    font-family: "Courier New", Courier, monospace;
    font-size: 13px;
    font-weight: 500;
    border-radius: 6px;
    padding: 3px 5px;
  }
`;
