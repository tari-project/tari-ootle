// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useAccountsList } from "@api/hooks/useAccounts";
import { useListTemplatesAuthored } from "@api/hooks/useTemplatesAuthored";
import PageHeading from "@components/PageHeading";
import { SelectChangeEvent, Stack } from "@mui/material";
import Grid from "@mui/material/Grid";
import { useTheme } from "@mui/material/styles";
import useAccountStore from "@store/accountStore";
import { AccountInfo, Type as FuncType, decodeOotleAddress, substateIdToString } from "@tari-project/ootle-ts-bindings";
import { useEffect, useState } from "react";
import TemplateList from "./components/TemplateList";
import Wrapper from "./components/Wrapper";

// TODO - move to helpers
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
      <Grid size={12}>
        <PageHeading>Templates</PageHeading>
      </Grid>
      <Wrapper>
        <Stack spacing={1}>
          <TemplateList />
        </Stack>
      </Wrapper>
    </>
  );
}

export default Templates;
