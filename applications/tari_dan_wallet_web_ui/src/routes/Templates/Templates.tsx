// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useListTemplatesAuthored} from "../../api/hooks/useTemplatesAuthored";
import {useEffect, useState} from "react";
import {useAccountsList} from "../../api/hooks/useAccounts";
import InputLabel from "@mui/material/InputLabel";
import Select, {SelectChangeEvent} from "@mui/material/Select/Select";
import {AccountInfo, substateIdToString} from "@tari-project/typescript-bindings";
import MenuItem from "@mui/material/MenuItem";
import FormControl from "@mui/material/FormControl";

function Templates() {
    const pageSize = 20;
    const [page, setPage] = useState(0);
    const [keyIndex, setKeyIndex] = useState(0);
    const [maxPage, setMaxPage] = useState(0);
    const {
        data: templatesResponse,
        isLoading: alreadyRegisteredIsLoading,
        isError: alreadyRegisteredIsError,
        error: alreadyRegisteredError,
    } = useListTemplatesAuthored({key_index: 0, page: page, page_size: pageSize});

    const {
        data: dataAccountsList,
        isLoading: isLoadingAccountsList,
        isError: isErrorAccountsList,
        error: errorAccountsList,
    } = useAccountsList(0, 10);

    const onAccountChange = (e: SelectChangeEvent<string>) => {
        const newKeyIndex: number = +e.target.value;
        setKeyIndex(newKeyIndex)
    };

    useEffect(() => {
        console.log("Current key index: ", keyIndex);
    }, [keyIndex]);

    return (<FormControl>
        <InputLabel id="account">Account</InputLabel>
        <Select
            labelId="account"
            name="account"
            label="Account"
            style={{ flexGrow: 1, minWidth: "200px" }}
            onChange={onAccountChange}
        >
            {dataAccountsList?.accounts.map((account: AccountInfo, index: number) => {
                return (
                    <MenuItem
                        key={substateIdToString(account.account.address)}
                        value={account.account.key_index}
                    >
                        {account.account.name}
                    </MenuItem>
                );
            })}
        </Select>
    </FormControl>);
}

export default Templates;