//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { useQuery } from "@tanstack/react-query";
import { RestClient } from "../client";

declare module "@tanstack/react-query" {
  interface Register {
    defaultError: Error;
  }
}
export const client = RestClient.new();


export const useDatabasesList = () => {
  return useQuery<{ name: string, path: string }[], Error>({
    queryKey: ["dbs"],
    queryFn: () => client.listDatabases().then((res) => res.databases),
    refetchOnMount: false,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: false,
  });
};

interface ListColumnFamiliesResponse {
  cfs: {
    name: string;
    num_entries: number;
    total_entries_bytes: number;
  }[];
  dir_size: number;
}

export const useDatabaseCfsList = (dbName: string) => {
  return useQuery<ListColumnFamiliesResponse, Error>({
    queryKey: ["dbs-cfs", dbName],
    queryFn: () => client.listColumnFamilies(dbName),
    refetchOnMount: false,
    refetchIntervalInBackground: false,
    refetchOnWindowFocus: false,
  });
};

