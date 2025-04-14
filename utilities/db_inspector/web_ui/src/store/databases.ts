//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { useQuery } from "@tanstack/react-query";
import { RestClient } from "../client";

export const client = RestClient.new();

export const useDatabasesList = () => {
  return useQuery({
    queryKey: ["dbs"],
    queryFn: () => client.listDatabases().then((res) => res.databases),
    refetchOnMount: false,
  });
};


export const useDatabaseCfsList = (dbName: string) => {
  return useQuery({
    queryKey: ["dbs-cfs", dbName],
    queryFn: () => client.listColumnFamilies(dbName).then((res) => res.cfs),
    refetchOnMount: false,
  });
};

