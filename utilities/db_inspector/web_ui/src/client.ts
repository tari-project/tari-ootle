//   Copyright 2025 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

const BASE_URL = (import.meta.env.VITE_API_ADDRESS || "http://localhost:9090/api").replace(/\/+$/, "");

export type Params = { [key: string]: string | number | boolean | null };

function getUrl(entity: string, params: Params = {}) {
  function toQueryString(params: Params): string {
    const searchParams = new URLSearchParams();
    for (const [key, value] of Object.entries(params)) {
      if (value !== null && value !== undefined) {
        searchParams.append(key, String(value));
      }
    }
    return searchParams.toString();
  }

  const queryString = toQueryString(params);
  return `${BASE_URL}/${entity}${queryString ? `?${queryString}` : ""}`;
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
    let errorBody: { status?: unknown; message?: unknown } | null = null;
    try {
      errorBody = await response.json();
    } catch {
      errorBody = null;
    }
    if (errorBody && typeof errorBody.message === "string" && errorBody.message) {
      const status = errorBody.status ?? response.status;
      throw new Error(`${status}: ${errorBody.message}`);
    }
    throw new Error(`Error: ${response.status}: ${response.statusText}`);
  }

  return await response.json();
}

export function RestClient() {
  return {
    listDatabases: async () => {
      return await getRequest("databases");
    },
    listColumnFamilies: async (dbName: string) => {
      return await getRequest(`databases/${encodeURIComponent(dbName)}/column-families`);
    },

    listCfItems: async (dbName: string, cfName: string, params: Params = {}) => {
      return await getRequest(
        `databases/${encodeURIComponent(dbName)}/column-families/${encodeURIComponent(cfName)}`,
        params,
      );
    },
  };
}

RestClient.new = () => {
  return RestClient();
};
