// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useQuery } from "@tanstack/react-query";
import { TemplatesListAuthoredRequest } from "@tari-project/ootle-ts-bindings";
import { templatesListAuthored } from "@utils/json_rpc";

export const useListTemplatesAuthored = (request: TemplatesListAuthoredRequest) => {
  return useQuery({
    queryKey: ["templates_list_authored", request],
    queryFn: () => {
      return templatesListAuthored(request);
    },
    refetchInterval: false,
    notifyOnChangeProps: ["data", "error"],
    retryOnMount: false,
    retry: false,
  });
};
