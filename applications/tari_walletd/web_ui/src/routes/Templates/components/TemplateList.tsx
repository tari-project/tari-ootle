import { useListTemplatesAuthored } from "@api/hooks/useTemplatesAuthored";
import FetchStatusCheck from "@components/FetchStatusCheck";
import { TableBody, TableCell, TableHead, TableRow } from "@mui/material";
import Table from "@mui/material/Table";
import TableContainer from "@mui/material/TableContainer";
import useAccountStore from "@store/accountStore";
import { decodeOotleAddress } from "@tari-project/ootle-ts-bindings";
import { Fragment, useState } from "react";

const COLUMNS = ["Address", "Name", "ABI Version", ""];

export default function TemplateList() {
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const ootleAddress = useAccountStore((s) => s.address);
  const address = ootleAddress ? decodeOotleAddress(ootleAddress) : null;
  const { data, isLoading, isError, error } = useListTemplatesAuthored({
    author_public_key: address?.accountPublicKey || "",
    page: page,
    page_size: rowsPerPage,
  });

  const headers = COLUMNS.map((c) => <TableCell key={c}>{c}</TableCell>);
  const templates = data?.templates.map((template) => {
    console.debug(template);
    return <Fragment key={template.address}></Fragment>;
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
    </FetchStatusCheck>
  );
}
