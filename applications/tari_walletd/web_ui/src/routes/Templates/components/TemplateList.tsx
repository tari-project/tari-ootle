import { useListTemplatesAuthored } from "@api/hooks/useTemplatesAuthored";
import CopyAddress from "@components/CopyAddress";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { AccordionIconButton, FluidTableCell } from "@components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import { TableBody, TableCell, TableHead, TablePagination, TableRow } from "@mui/material";
import Table from "@mui/material/Table";
import TableContainer from "@mui/material/TableContainer";
import TemplateItem from "@routes/Templates/components/TemplateItem";
import useAccountStore from "@store/accountStore";
import { decodeOotleAddress } from "@tari-project/ootle-ts-bindings";
import { handleChangePage, handleChangeRowsPerPage } from "@utils/helpers";
import { Fragment, useState } from "react";

const COLUMNS = ["Address", "Name", "ABI Version", ""];

export default function TemplateList() {
  const ootleAddress = useAccountStore((s) => s.address);
  const address = ootleAddress ? decodeOotleAddress(ootleAddress) : null;

  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [openItem, setOpenItem] = useState<string | undefined>();

  const { data, isLoading, isError, error } = useListTemplatesAuthored({
    author_public_key: address?.accountPublicKey || "",
    page: page,
    page_size: rowsPerPage,
  });

  const headers = COLUMNS.map((c) => <TableCell key={c}>{c}</TableCell>);

  function handleExpandClick(address: string) {
    setOpenItem((c) => (c === address ? undefined : address));
  }
  const templates = data?.templates.slice(1, 2).map((template) => {
    const { address, name, abi_version } = template;
    const isOpen = address === openItem;
    return (
      <Fragment key={`${name}-${address}`}>
        <TableRow>
          <FluidTableCell>
            <CopyAddress address={`template_${address}`} />
          </FluidTableCell>
          <FluidTableCell>{name}</FluidTableCell>
          <FluidTableCell>{abi_version}</FluidTableCell>
          <FluidTableCell>
            <AccordionIconButton aria-label="expand row" size="small" onClick={() => handleExpandClick(address)}>
              {isOpen ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
            </AccordionIconButton>
          </FluidTableCell>
        </TableRow>
        {isOpen && <TemplateItem template={template} isOpen={isOpen} />}
      </Fragment>
    );
  });

  return (
    <FetchStatusCheck
      isError={isError}
      isLoading={isLoading}
      errorMessage={error ? (error as Error).message : "Error fetching templates."}
    >
      <TableContainer>
        <Table>
          <TableHead>
            <TableRow>{headers}</TableRow>
          </TableHead>
          <TableBody>{templates}</TableBody>
        </Table>
      </TableContainer>
      <TablePagination
        rowsPerPageOptions={[1, 25, 50]}
        component="div"
        count={data?.total_templates || 0}
        rowsPerPage={rowsPerPage}
        page={page}
        onPageChange={(event, newPage) => handleChangePage(event, newPage, setPage)}
        onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setRowsPerPage, setPage)}
      />
    </FetchStatusCheck>
  );
}
