// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useQuery } from "@tanstack/react-query";
import { ApiError } from "../helpers/types";
import { webauthnAlreadyRegistered } from "../../utils/json_rpc";

export const useWebauthnAlreadyRegistered = (username: string) => {
  return useQuery({
    queryKey: ["webauthn_already_registered", username],
    queryFn: () => {
      return webauthnAlreadyRegistered(username);
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
