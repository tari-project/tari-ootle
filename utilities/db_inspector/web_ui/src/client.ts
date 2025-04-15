//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

const URL = import.meta.env.VITE_API_ADDRESS || "http://localhost:9090/api";

type Params = string[][] | Record<string, string | number | boolean> | string | URLSearchParams;

function getUrl(entity: string, params: Params = {}) {
  const queryString = new URLSearchParams(params).toString();
  return `${URL}/${entity}${queryString ? `?${queryString}` : ""}`;
}

async function getRequest(path: string, params: Params = {}) {
  const headers = {
    "Content-Type": "application/json",
  };
  const response = await fetch(getUrl(path, params), {
    method: "GET",
    headers,
  });

  if (!response.ok) {
    try {
      const resp = await response.json();
      throw new Error(`${resp.status}: ${resp.message}`);
    } catch (e) {
      throw new Error(`Error: ${response.status}: ${response.statusText} ${e}`);
    }
  }

  return await response.json();
}


export function RestClient() {
  return {
    listDatabases: async () => {
      return await getRequest("databases");
    },
    listColumnFamilies: async (dbName: string) => {
      return await getRequest("databases/" + dbName + "/column-families");
    },

    listCfItems: async (dbName: string, cfName: string, params: Params = {}) => {
      return await getRequest(`databases/${dbName}/column-families/${cfName}`, params);
    },

  };
}

RestClient.new = () => {
  return RestClient();
};