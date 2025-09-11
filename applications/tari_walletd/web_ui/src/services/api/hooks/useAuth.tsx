// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useQuery } from "@tanstack/react-query";
import { authGetMethod } from "@utils/json_rpc";

export const useAuthMethod = () => {
  return useQuery({
    queryKey: ["auth_method"],
    queryFn: () => {
      return authGetMethod();
    },
  });
};
