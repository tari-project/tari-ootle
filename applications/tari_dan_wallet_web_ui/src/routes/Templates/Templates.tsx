// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useListTemplatesAuthored} from "../../api/hooks/useTemplatesAuthored";
import {useEffect, useState} from "react";
import {useAccountsList} from "../../api/hooks/useAccounts";
import InputLabel from "@mui/material/InputLabel";
import Select, {SelectChangeEvent} from "@mui/material/Select/Select";
import {
    Account,
    AccountInfo,
    ArgDef,
    AuthoredTemplate,
    type FunctionDef,
    substateIdToString,
    Type as FuncType
} from "@tari-project/typescript-bindings";
import MenuItem from "@mui/material/MenuItem";
import FormControl from "@mui/material/FormControl";
import Table from "@mui/material/Table";
import TableHead from "@mui/material/TableHead";
import TableRow from "@mui/material/TableRow";
import TableCell from "@mui/material/TableCell";
import TableBody from "@mui/material/TableBody";
import TableContainer from "@mui/material/TableContainer";
import CopyAddress from "../../Components/CopyAddress";
import {AccordionIconButton, DataTableCell} from "../../Components/StyledComponents";
import Grid from "@mui/material/Grid";
import KeyboardArrowUpIcon from "@mui/icons-material/KeyboardArrowUp";
import KeyboardArrowDownIcon from "@mui/icons-material/KeyboardArrowDown";
import {Collapse, TablePagination} from "@mui/material";
import {useTheme} from "@mui/material/styles";
import {SlCheck, SlClose} from "react-icons/sl";
import {handleChangePage, handleChangeRowsPerPage} from "../../utils/helpers";

function getTypeAsString(funcType: FuncType): any {
    if (typeof funcType === 'string') {
        return funcType;
    }

    const funcTypeKeys = Object.keys(funcType);
    if (funcTypeKeys.length > 0) {
        switch (funcTypeKeys[0]) {
            case "Vec": {
                // @ts-ignore
                return getTypeAsString(funcType["Vec"]);
            }
            case "Tuple": {
                // @ts-ignore
                return JSON.stringify(funcType["Tuple"]);
            }
            case "Other": {
                // @ts-ignore
                return funcType["Other"]["name"];
            }
        }
    }

    return "Unknown";
}

export interface TemplatesProps {
    account?: Account,
}

function Templates({ props }: { props: TemplatesProps }) {
    const [page, setPage] = useState(0);
    const [templatesCount, setTemplatesCount] = useState(0);
    const [keyIndex, setKeyIndex] = useState(0);
    const [account, setAccount] = useState("");
    const [open, setOpen] = useState<boolean[]>([]);
    const [rowsPerPage, setRowsPerPage] = useState(10);
    const theme = useTheme();
    const {
        data: templatesResponse,
    } = useListTemplatesAuthored({key_index: keyIndex, page: page, page_size: rowsPerPage});

    const {
        data: dataAccountsList,
    } = useAccountsList(0, 10);

    useEffect(() => {
        if (props && props.account) {
            setAccount(props.account.key_index.toString());
            setKeyIndex(props.account.key_index);
        }
    }, [props]);

    const onAccountChange = (e: SelectChangeEvent<string>) => {
        const newKeyIndex: number = +e.target.value;
        setKeyIndex(newKeyIndex);
        console.log("target value:", e.target.value);
        setAccount(e.target.value);
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
        <Grid item xs={12} md={12} lg={12}>
            {!props || !props.account ? <h2>Templates</h2> : null}

            {!props || !props.account ?
                <FormControl>
        <InputLabel id="account">Account</InputLabel>
        <Select
            labelId="account"
            name="account"
            label="Account"
            style={{ flexGrow: 1, minWidth: "200px" }}
            value={account}
            onChange={onAccountChange}
        >
            {dataAccountsList?.accounts.map((account: AccountInfo, index: number) => {
                return (
                    <MenuItem
                        key={substateIdToString(account.account.address)}
                        value={account.account.key_index.toString()}
                    >
                        {account.account.name}
                    </MenuItem>
                );
            })}
        </Select>
        </FormControl> : null}

        {account ?
            (<Grid item xs={12} md={12} lg={12}>
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
                                    <CopyAddress address={template.address} />
                                </DataTableCell>
                                <DataTableCell>
                                    {template.name}
                                </DataTableCell>
                                <TableCell>
                                    {template.tari_version}
                                </TableCell>
                                <TableCell>
                                <AccordionIconButton
                                    aria-label="expand row"
                                    size="small"
                                    onClick={() => {
                                        if (open) {
                                            let newOpen: boolean[] = [];
                                            open.forEach((value, idx) => {
                                                if (idx == index) {
                                                    value = !value;
                                                }
                                                newOpen[idx] = value;
                                            });
                                            setOpen(newOpen);
                                        }
                                    }}
                                >
                                    {open[index] ? <KeyboardArrowUpIcon /> : <KeyboardArrowDownIcon />}
                                </AccordionIconButton>
                                </TableCell>
                            </TableRow>
                            {open[index] ?
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
                                                                            <TableCell>{
                                                                                funcDef.is_mut
                                                                                ? <SlCheck size={25} color={"green"} />
                                                                                : <SlClose size={25} color={"red"} />
                                                                            }
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
                                                                                ) : <SlClose size={25} color={"red"} />}
                                                                            </TableCell>
                                                                            <TableCell>{getTypeAsString(funcDef.output)}</TableCell>
                                                                        </TableRow>
                                                                    )
                                                                })}
                                                            </TableBody>
                                                        </Table>
                                                    </TableContainer>
                                            ) : null}
                                        </Collapse>
                                    </DataTableCell>
                                </TableRow>
                                : null
                            }
                            </>
                        )
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
            </Grid>) : null }
        </Grid>
    );
}

export default Templates;