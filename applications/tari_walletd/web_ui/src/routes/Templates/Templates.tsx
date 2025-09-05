// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useListTemplatesAuthored } from "../../api/hooks/useTemplatesAuthored";
import { useEffect, useState } from "react";
import { useAccountsList } from "../../api/hooks/useAccounts";
import InputLabel from "@mui/material/InputLabel";
import Select, { SelectChangeEvent } from "@mui/material/Select/Select";
import {
  Account,
  AccountInfo,
  ArgDef,
  AuthoredTemplate,
  type FunctionDef,
  substateIdToString,
  Type as FuncType,
} from "@tari-project/typescript-bindings";
import MenuItem from "@mui/material/MenuItem";
import FormControl from "@mui/material/FormControl";
import Table from "@mui/material/Table";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TableCell from "@mui/material/TableCell";
import TableBody from "@mui/material/TableBody";
import TableContainer from "@mui/material/TableContainer";
import CopyAddress from "@components/CopyAddress";
import { AccordionIconButton, DataTableCell } from "@components/StyledComponents";
import Grid from "@mui/material/Grid";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import { Collapse, TablePagination } from "@mui/material";
import { useTheme } from "@mui/material/styles";
import { SlCheck, SlClose } from "react-icons/sl";
import { handleChangePage, handleChangeRowsPerPage } from "../../utils/helpers";
import useAccountStore from "../../store/accountStore";

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
  const { data: templatesResponse } = useListTemplatesAuthored({
    author_public_key: account?.public_key || accountStore.publicKey,
    page: page,
    page_size: rowsPerPage,
  });

  const { data: dataAccountsList, isLoading: isAccountsLoading } = useAccountsList(0, 10);

  useEffect(() => {
    const defaultAcc = dataAccountsList?.accounts.find((account: AccountInfo) => account.account.is_default);
    setAccount(defaultAcc);
  }, [dataAccountsList]);

  const onAccountChange = (e: SelectChangeEvent<string>) => {
    const selected = dataAccountsList?.accounts.find(
      (account: AccountInfo) => substateIdToString(account.account.address) === e.target.value,
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

  if (isAccountsLoading) {
    return <div>Loading...</div>;
  }

  return (
    <Grid item xs={12} md={12} lg={12}>
      {<h2>Templates</h2>}

      {account ? (
        <FormControl>
          <InputLabel id="account">Account</InputLabel>
          <Select
            labelId="account"
            name="account"
            label="Account"
            style={{ flexGrow: 1, minWidth: "200px" }}
            value={substateIdToString(account.account.address)}
            onChange={onAccountChange}
          >
            {dataAccountsList?.accounts.map((account: AccountInfo, index: number) => {
              return (
                <MenuItem
                  key={substateIdToString(account.account.address)}
                  value={substateIdToString(account.account.address)}
                >
                  {account.account.name || substateIdToString(account.account.address)}
                </MenuItem>
              );
            })}
          </Select>
        </FormControl>
      ) : null}

      {account ? (
        <Grid item xs={12} md={12} lg={12}>
          <TableContainer>
            <Table>
              <TableHead>
                <TableRow>
                  <TableCell>Address</TableCell>
                  <TableCell>Name</TableCell>
                  <TableCell>Tari Version</TableCell>
                  <TableCell></TableCell>
                </TableRow>
              </TableHead>
              <TableBody>
                {templatesResponse?.templates.map((template: AuthoredTemplate, index: number) => {
                  return (
                    <>
                      <TableRow key={`template-${index}-1`}>
                        <DataTableCell>
                          <CopyAddress address={`template_${template.address}`} />
                        </DataTableCell>
                        <DataTableCell>{template.name}</DataTableCell>
                        <TableCell>{template.tari_version}</TableCell>
                        <TableCell>
                          <AccordionIconButton
                            aria-label="expand row"
                            size="small"
                            onClick={() => {
                              if (open) {
                                const newOpen = open.map((value, idx) => (idx === index ? !value : value));
                                setOpen(newOpen);
                              }
                            }}
                          >
                            {open[index] ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
                          </AccordionIconButton>
                        </TableCell>
                      </TableRow>
                      {open[index] ? (
                        <TableRow key={`template-${index}-open`}>
                          <DataTableCell
                            style={{
                              paddingBottom: theme.spacing(1),
                              paddingTop: 0,
                              borderBottom: "none",
                            }}
                            colSpan={2}
                          >
                            <Collapse in={open[index]} timeout="auto" unmountOnExit>
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
                                              {funcDef.is_mut ? (
                                                <SlCheck size={25} color={"green"} />
                                              ) : (
                                                <SlClose size={25} color={"red"} />
                                              )}
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
                    </>
                  );
                })}
              </TableBody>
            </Table>
          </TableContainer>
          <Grid item xs={12} md={12} lg={12}>
            <TablePagination
              rowsPerPageOptions={[10, 25, 50]}
              component="div"
              count={templatesCount}
              rowsPerPage={rowsPerPage}
              page={page}
              onPageChange={(event, newPage) => handleChangePage(event, newPage, setPage)}
              onRowsPerPageChange={(event) => handleChangeRowsPerPage(event, setRowsPerPage, setPage)}
            />
          </Grid>
        </Grid>
      ) : null}
    </Grid>
  );
}

export default Templates;
