//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

let nextId = 0;

async function call(address: string, method: string, params: unknown): Promise<any> {
  const response = await fetch(address, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: (nextId += 1), method, params }),
  });
  if (!response.ok) {
    throw new Error(`${method} failed: ${response.status} ${response.statusText}`);
  }
  const json = await response.json();
  if (json.error) {
    throw new Error(json.error.message ?? `${method} failed`);
  }
  return json.result;
}

const DAEMON_ADDRESS = import.meta.env.VITE_JSON_RPC_ADDRESS || import.meta.env.VITE_JRPC_ADDRESS || "/json_rpc";

/** Calls the swarm daemon that serves this UI. */
export function swarmRpc(method: string, params: unknown = {}): Promise<any> {
  return call(DAEMON_ADDRESS, method, params);
}

/** Calls one managed instance's own JSON-RPC endpoint. */
export function nodeRpc(url: string, method: string, params: unknown = null): Promise<any> {
  return call(url, method, params);
}

export function describeError(err: unknown): string {
  if (err instanceof Error) {
    return err.message;
  }
  if (typeof err === "string") {
    return err;
  }
  if (err && typeof err === "object" && "message" in err) {
    return String((err as { message: unknown }).message);
  }
  return "Request failed";
}
