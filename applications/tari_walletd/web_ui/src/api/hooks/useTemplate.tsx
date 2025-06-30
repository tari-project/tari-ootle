// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useQuery } from "@tanstack/react-query";
import { ApiError } from "../helpers/types";
import { templatesGet } from "../../utils/json_rpc";
import { TemplatesGetRequest } from "@tari-project/typescript-bindings";

export const useTemplateGet = (request: TemplatesGetRequest, options = {}) => {
  return useQuery({
    queryKey: ["template_get", request],
    queryFn: () => {
      return templatesGet(request);
    },
    onError: (error: ApiError) => {
      error;
    },
    refetchInterval: false,
    notifyOnChangeProps: ["data", "error"],
    retryOnMount: false,
    retry: false,
    ...options,
  });
};
