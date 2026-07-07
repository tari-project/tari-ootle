//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { useQuery } from "@tanstack/react-query";
import { getNetworkEconomics } from "../../utils/api";

export const useNetworkEconomics = () => {
  return useQuery({
    queryKey: ["network_economics"],
    queryFn: getNetworkEconomics,
    refetchInterval: 30 * 1000,
  });
};
