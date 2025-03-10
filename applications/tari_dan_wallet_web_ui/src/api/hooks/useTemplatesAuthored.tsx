// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useQuery} from "@tanstack/react-query";
import {ApiError} from "../helpers/types";
import {templatesListAuthored} from "../../utils/json_rpc";
import {TemplatesListAuthoredRequest} from "@tari-project/typescript-bindings";

export const useListTemplatesAuthored = (request: TemplatesListAuthoredRequest) => {
  return useQuery({
    queryKey: ["templates_list_authored", request],
    queryFn: () => {
      return templatesListAuthored(request);
    },
    onError: (error: ApiError) => {
      error;
    },
    refetchInterval: false,
    notifyOnChangeProps: ["data", "error"],
    retryOnMount: false,
    retry: false,
  });
};
