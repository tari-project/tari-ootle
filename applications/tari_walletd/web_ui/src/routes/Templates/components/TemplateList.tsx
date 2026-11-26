//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause
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
import { handleChangePage, handleChangeRowsPerPage } from "@utils/helpers";
import { Dispatch, Fragment, SetStateAction, useState } from "react";
import { buildAuthoredTemplatesQuery, extractAuthorPublicKey } from "./templateListHelpers";

const COLUMNS = ["Address", "Name", "ABI Version", ""];

export default function TemplateList() {
  const ootleAddress = useAccountStore((s) => s.address);
  const authorPublicKey = extractAuthorPublicKey(ootleAddress);
  const [rowsPerPage, setRowsPerPage] = useState(10);

  return (
    <TemplateListContent
      key={authorPublicKey ?? "none"}
      authorPublicKey={authorPublicKey}
      rowsPerPage={rowsPerPage}
      setRowsPerPage={setRowsPerPage}
    />
  );
}

function TemplateListContent({
  authorPublicKey,
  rowsPerPage,
  setRowsPerPage,
}: {
  authorPublicKey: string | null;
  rowsPerPage: number;
  setRowsPerPage: Dispatch<SetStateAction<number>>;
}) {
  const [page, setPage] = useState(0);
  const [openItem, setOpenItem] = useState<string | undefined>();

  const { request, enabled } = buildAuthoredTemplatesQuery(authorPublicKey, page, rowsPerPage);
  const { data, isLoading, isError, error } = useListTemplatesAuthored(request, { enabled });

  const headers = COLUMNS.map((c) => <TableCell key={c}>{c}</TableCell>);

  function handleExpandClick(address: string) {
    setOpenItem((c) => (c === address ? undefined : address));
  }
  const templates = data?.templates.map((template) => {
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
        rowsPerPageOptions={[10, 25, 50]}
        component="div"
        count={Number(data?.total_templates) || 0}
        rowsPerPage={rowsPerPage}
        page={page}
        onPageChange={(event, newPage) => handleChangePage(event, newPage, setPage)}
        onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setRowsPerPage, setPage)}
      />
    </FetchStatusCheck>
  );
}
