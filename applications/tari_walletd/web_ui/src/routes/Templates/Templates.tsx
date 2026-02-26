// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useAccountsList } from "@api/hooks/useAccounts";
import { useListTemplatesAuthored } from "@api/hooks/useTemplatesAuthored";
import CopyAddress from "@components/CopyAddress";
import FetchStatusCheck from "@components/FetchStatusCheck";
import PageHeading from "@components/PageHeading";
import { AccordionIconButton, DataTableCell, StyledPaper } from "@components/StyledComponents";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import {
  Collapse,
  FormControl,
  InputLabel,
  MenuItem,
  Select,
  SelectChangeEvent,
  Stack,
  Table,
  TableBody,
  TableCell,
  TableContainer,
  TableHead,
  TablePagination,
  TableRow,
} from "@mui/material";
import Grid from "@mui/material/Grid";
import { useTheme } from "@mui/material/styles";
import useAccountStore from "@store/accountStore";
import {
  AccountInfo,
  ArgDef,
  AuthoredTemplate,
  Type as FuncType,
  type FunctionDef,
  decodeOotleAddress,
  substateIdToString,
} from "@tari-project/ootle-ts-bindings";
import { handleChangePage, handleChangeRowsPerPage } from "@utils/helpers";
import { Fragment, useEffect, useState } from "react";
import { SlCheck, SlClose } from "react-icons/sl";

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

function Templates() {
  const [page, setPage] = useState(0);
  const [templatesCount, setTemplatesCount] = useState(0);
  const accountStore = useAccountStore();
  const [account, setAccount] = useState<AccountInfo | undefined>(undefined);
  const [open, setOpen] = useState<boolean[]>([]);
  const [rowsPerPage, setRowsPerPage] = useState(10);
  const theme = useTheme();
  const a = account?.address || accountStore.address;
  const address = a ? decodeOotleAddress(a) : null;
  const {
    data: templatesResponse,
    isLoading,
    isError,
    error,
  } = useListTemplatesAuthored({
    author_public_key: address?.accountPublicKey || "",
    page: page,
    page_size: rowsPerPage,
  });

  const { data: dataAccountsList, isLoading: isAccountsLoading } = useAccountsList(0, 10);

  useEffect(() => {
    const defaultAcc = dataAccountsList?.accounts.find((account: AccountInfo) => account.account.is_default);
    setAccount(defaultAcc);
  }, [dataAccountsList]);

  const onAccountChange = (e: SelectChangeEvent) => {
    const selected = dataAccountsList?.accounts.find(
      (account: AccountInfo) => substateIdToString(account.account.component_address) === e.target.value,
    );
    setAccount(selected);
  };

  useEffect(() => {
    if (templatesResponse && templatesResponse.templates.length > 0) {
      let opens = new Array<boolean>(templatesResponse.templates.length);
      opens.fill(false);
      setOpen(opens);
      setTemplatesCount(templatesResponse.total_templates);
    }
  }, [templatesResponse]);

  return (
    <>
      <Grid item xs={12} md={12} lg={12}>
        <PageHeading>Templates</PageHeading>
      </Grid>
      <Grid item xs={12} md={12} lg={12}>
        <StyledPaper>
          <FetchStatusCheck
            isLoading={isLoading || isAccountsLoading}
            isError={isError}
            errorMessage={error ? (error as Error).message : "Error fetching templates."}
          >
            <Stack spacing={1}>
              <Stack alignItems="center" justifyContent="flex-end" direction="row" spacing={2}>
                {account ? (
                  <FormControl style={{ minWidth: "250px" }}>
                    <InputLabel id="account">Account</InputLabel>
                    <Select
                      labelId="account"
                      name="account"
                      label="Account"
                      value={substateIdToString(account.account.component_address)}
                      onChange={onAccountChange}
                      size="medium"
                    >
                      {dataAccountsList?.accounts.map((account: AccountInfo, i: number) => {
                        return (
                          <MenuItem key={i} value={substateIdToString(account.account.component_address)}>
                            {account.account.name || substateIdToString(account.account.component_address)}
                          </MenuItem>
                        );
                      })}
                    </Select>
                  </FormControl>
                ) : null}
              </Stack>
              {account ? (
                <TableContainer>
                  <Table>
                    <TableHead>
                      <TableRow>
                        <TableCell>Address</TableCell>
                        <TableCell>Name</TableCell>
                        <TableCell>ABI Version</TableCell>
                        <TableCell></TableCell>
                      </TableRow>
                    </TableHead>
                    <TableBody>
                      {templatesResponse?.templates.map((template: AuthoredTemplate, i: number) => {
                        return (
                          <Fragment key={i}>
                            <TableRow>
                              <DataTableCell>
                                <CopyAddress address={`template_${template.address}`} />
                              </DataTableCell>
                              <DataTableCell>{template.name}</DataTableCell>
                              <TableCell>{template.abi_version}</TableCell>
                              <TableCell>
                                <AccordionIconButton
                                  aria-label="expand row"
                                  size="small"
                                  onClick={() => {
                                    if (open) {
                                      const newOpen = open.map((value, idx) => (idx === i ? !value : value));
                                      setOpen(newOpen);
                                    }
                                  }}
                                >
                                  {open[i] ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
                                </AccordionIconButton>
                              </TableCell>
                            </TableRow>
                            {open[i] ? (
                              <TableRow>
                                <DataTableCell
                                  style={{
                                    paddingBottom: theme.spacing(1),
                                    paddingTop: 0,
                                    borderBottom: "none",
                                  }}
                                  colSpan={2}
                                >
                                  <Collapse in={open[i]} timeout="auto" unmountOnExit>
                                    <h3>Functions</h3>
                                    {template.functions ? (
                                      <TableContainer>
                                        <Table>
                                          <TableHead>
                                            <TableRow>
                                              <TableCell>Name</TableCell>
                                              <TableCell>Mutable</TableCell>
                                              <TableCell>Arguments</TableCell>
                                              <TableCell>Output</TableCell>
                                            </TableRow>
                                          </TableHead>
                                          <TableBody>
                                            {template.functions.map((funcDef: FunctionDef, index: number) => {
                                              return (
                                                <TableRow key={index}>
                                                  <TableCell>{funcDef.name}</TableCell>
                                                  <TableCell>
                                                    {funcDef.is_mut ? <SlCheck size={25} /> : <SlClose size={25} />}
                                                  </TableCell>
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
                                </DataTableCell>
                              </TableRow>
                            ) : null}
                          </Fragment>
                        );
                      })}
                    </TableBody>
                  </Table>
                </TableContainer>
              ) : null}
              <TablePagination
                rowsPerPageOptions={[10, 25, 50]}
                component="div"
                count={templatesCount}
                rowsPerPage={rowsPerPage}
                page={page}
                onPageChange={(event, newPage) => handleChangePage(event, newPage, setPage)}
                onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setRowsPerPage, setPage)}
              />
            </Stack>
          </FetchStatusCheck>
        </StyledPaper>
      </Grid>
    </>
  );
}

export default Templates;
