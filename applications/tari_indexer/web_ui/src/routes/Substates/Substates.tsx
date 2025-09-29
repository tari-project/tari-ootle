//  Copyright 2024. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import PageHeading from "../../Components/PageHeading";
import Grid from "@mui/material/Grid";
import { StyledPaper, DataTableCell } from "../../Components/StyledComponents";
import {
  Box,
  Button,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableRow,
  TextField,
  Select,
  MenuItem,
  TableContainer,
  TablePagination,
  FormControl,
  InputLabel,
} from "@mui/material";
import React, { useEffect, useState, useMemo } from "react";
import { truncateText } from "../../utils/helpers";
import CopyToClipboard from "../../Components/CopyToClipboard";
import saveAs from "file-saver";
import JsonDialog from "../../Components/JsonDialog";
import { ListSubstateItem, shortenSubstateId, substateIdToString } from "@tari-project/typescript-bindings";
import { listSubstates, getSubstate } from "../../utils/json_rpc";
import { Link } from "react-router-dom";
import FetchStatusCheck from "../../Components/FetchStatusCheck";

const SUBSTATE_TYPES = [
  "Component",
  "Resource",
  "Vault",
  "ClaimedOutputTombstone",
  "NonFungible",
  "TransactionReceipt",
  "ValidatorFeePool",
  "Template",
] as const;

type ExtendedSubstateItem = ListSubstateItem & { id: string; show?: boolean };

function SubstatesLayout() {
  const [substates, setSubstates] = useState<ListSubstateItem[]>([]);
  const [filteredSubstates, setFilteredSubstates] = useState<ExtendedSubstateItem[]>([]);
  const [page, setPage] = useState(0);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const [jsonDialogOpen, setJsonDialogOpen] = React.useState(false);
  const [selectedContent, setSelectedContent] = useState({});
  const [isLoading, setIsLoading] = useState(false);
  const [isError, setIsError] = useState(false);
  const [error, setError] = useState<Error | null>(null);
  const [filter, setFilter] = useState({
    filter_by_template: "",
    filter_by_type: "",
  });

  const extendedSubstates = useMemo(
    () => substates.map((substate) => ({ ...substate, id: substateIdToString(substate.substate_id) })),
    [substates]
  );

  const visibleSubstates = filteredSubstates.filter((substate) => substate.show !== false);
  const paginatedSubstates = visibleSubstates.slice(page * rowsPerPage, page * rowsPerPage + rowsPerPage);

  useEffect(() => {
    setFilteredSubstates(extendedSubstates);
  }, [extendedSubstates]);

  useEffect(() => {
    get_substates(0, 50, { filter_by_template: null, filter_by_type: null });
  }, []);

  async function get_substates(offset: number, limit: number, filter: any) {
    setIsLoading(true);
    setIsError(false);
    setError(null);

    try {
      let params = {
        limit,
        offset,
        filter_by_template: null,
        filter_by_type: null,
      };
      if (filter.filter_by_template) {
        params.filter_by_template = filter.filter_by_template;
      }
      if (filter.filter_by_type) {
        params.filter_by_type = filter.filter_by_type;
      }

      // Ignoring eslint about BintInt to number conversion, as BigInts break serialization
      // @ts-ignore
      let resp = await listSubstates(params);
      setSubstates(resp.substates);
    } catch (err) {
      setIsError(true);
      setError(err as Error);
    } finally {
      setIsLoading(false);
    }
  }

  const handleChangePage = (_event: React.MouseEvent<HTMLButtonElement> | null, newPage: number) => {
    setPage(newPage);
  };

  const handleChangeRowsPerPage = (event: React.ChangeEvent<HTMLInputElement | HTMLTextAreaElement>) => {
    setRowsPerPage(parseInt(event.target.value, 10));
    setPage(0);
  };

  const handleContentDownload = async (substate: any) => {
    const data = await getSubstate({
      address: substate.substate_id,
      version: null,
      local_search_only: false,
    });

    const json = JSON.stringify(data, null, 2);
    const blob = new Blob([json], { type: "application/json" });
    const filename = `substates-${substate.address}-${substate.version}.json`;
    saveAs(blob, filename);
  };

  const handleContentView = async (substate: any) => {
    const data = await getSubstate({
      address: substate.substate_id,
      version: null,
      local_search_only: false,
    });
    setSelectedContent(data);
    setJsonDialogOpen(true);
  };

  const handleJsonDialogClose = () => {
    setJsonDialogOpen(false);
  };

  const onFilterChange = async (e: React.ChangeEvent<HTMLInputElement>) => {
    const newFilter = {
      ...filter,
      [e.target.name]: e.target.value,
    };

    setFilter(newFilter);

    const offset = 0;
    await get_substates(offset, 50, {
      filter_by_template: newFilter.filter_by_template || null,
      filter_by_type: newFilter.filter_by_type || null,
    });
    setPage(0);
  };

  return (
    <>
      <Grid item sm={12} md={12} xs={12}>
        <PageHeading>Substates</PageHeading>
      </Grid>
      <Grid item sm={12} md={12} xs={12}>
        <StyledPaper>
          <FetchStatusCheck
            isLoading={isLoading}
            isError={isError}
            errorMessage={error ? error.message : "Error fetching substates."}
          >
            <Stack spacing={1}>
              <Box className="flex-container" sx={{ marginBottom: 2 }}>
                <FormControl style={{ minWidth: "250px" }}>
                  <InputLabel shrink>Type</InputLabel>
                  <Select
                    name="filter_by_type"
                    label="Type"
                    value={filter.filter_by_type}
                    displayEmpty
                    onChange={async (e: any) => onFilterChange(e)}
                    size="medium"
                    renderValue={(value) => {
                      if (value === "") {
                        return "All types";
                      }
                      return value;
                    }}
                  >
                    <MenuItem key={"All Types"} value="">
                      {"All types"}
                    </MenuItem>
                    {SUBSTATE_TYPES.map((type) => (
                      <MenuItem key={type} value={type}>
                        {type}
                      </MenuItem>
                    ))}
                  </Select>
                </FormControl>
                <TextField
                  name="filter_by_template"
                  label="Template"
                  value={filter.filter_by_template}
                  onChange={async (e: any) => onFilterChange(e)}
                  style={{ flexGrow: 1 }}
                />
              </Box>
              <TableContainer>
                <Table>
                  <TableHead>
                    <TableRow>
                      <TableCell>Address</TableCell>
                      <TableCell>Version</TableCell>
                      <TableCell>Template</TableCell>
                      <TableCell>Timestamp</TableCell>
                      <TableCell>Content</TableCell>
                    </TableRow>
                  </TableHead>
                  <TableBody>
                    {paginatedSubstates.map((row) => (
                      <TableRow
                        key={substateIdToString(row.substate_id)}
                        sx={{ "&:last-child td, &:last-child th": { border: 0 } }}
                      >
                        <DataTableCell>
                          {substateIdToString(row.substate_id).startsWith("resource_") ? (
                            <Link to={`/resources/${substateIdToString(row.substate_id)}`}>
                              {shortenSubstateId(row.substate_id)}
                            </Link>
                          ) : (
                            shortenSubstateId(row.substate_id)
                          )}
                          <CopyToClipboard copy={substateIdToString(row.substate_id)} />
                        </DataTableCell>
                        <DataTableCell>{row.version}</DataTableCell>
                        <DataTableCell>
                          {row.template_address !== null && (
                            <>
                              {truncateText(row.template_address, 20)}
                              <CopyToClipboard copy={row.template_address} />
                            </>
                          )}
                        </DataTableCell>
                        <DataTableCell>{new Date(Number(row.timestamp) * 1000).toDateString()}</DataTableCell>
                        <DataTableCell>
                          <Stack direction="row" spacing={2} alignItems="left">
                            <Button variant="outlined" onClick={() => handleContentView(row)}>
                              View
                            </Button>
                            <Button variant="outlined" onClick={() => handleContentDownload(row)}>
                              Download
                            </Button>
                          </Stack>
                        </DataTableCell>
                      </TableRow>
                    ))}
                  </TableBody>
                </Table>
              </TableContainer>
              <TablePagination
                component="div"
                count={visibleSubstates.length}
                page={page}
                onPageChange={handleChangePage}
                rowsPerPage={rowsPerPage}
                onRowsPerPageChange={handleChangeRowsPerPage}
                rowsPerPageOptions={[5, 10, 25, 50]}
              />
            </Stack>
          </FetchStatusCheck>
        </StyledPaper>
      </Grid>
      <JsonDialog open={jsonDialogOpen} onClose={handleJsonDialogClose} data={selectedContent} />
    </>
  );
}

export default SubstatesLayout;
