//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

const URL = import.meta.env.VITE_API_ADDRESS || "http://localhost:9090/api";

export type Params = { [key: string]: string | number | boolean | null };

function getUrl(entity: string, params: Params = {}) {
  function toQueryString(params: Params): string {
    return new URLSearchParams(
      Object.entries(params).reduce((acc, [key, value]) => {
        acc[key] = String(value);
        return acc;
      }, {} as Record<string, string>),
    ).toString();
  }

  const queryString = toQueryString(params);
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