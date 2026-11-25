//  Copyright 2024 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

let nextId = 0;

function formatRpcError(error: unknown, method: string): string {
  if (typeof error === "string" && error.trim()) {
    return error.trim();
  }
  if (error && typeof error === "object") {
    const { message, code } = error as { message?: unknown; code?: unknown };
    if (typeof message === "string" && message.trim()) {
      return message.trim();
    }
    if (typeof code === "number" || typeof code === "string") {
      return `${method} failed (code ${code})`;
    }
  }
  return `${method} failed`;
}

async function call(address: string, method: string, params: unknown): Promise<any> {
  const response = await fetch(address, {
    method: "POST",
    headers: { "Content-Type": "application/json" },
    body: JSON.stringify({ jsonrpc: "2.0", id: (nextId += 1), method, params }),
  });
  if (!response.ok) {
    const status = [response.status, response.statusText].filter(Boolean).join(" ");
    throw new Error(`${method} failed: ${status}`);
  }
  let json: unknown;
  try {
    json = await response.json();
  } catch {
    throw new Error(`${method} failed: invalid JSON response`);
  }
  if (!json || typeof json !== "object") {
    throw new Error(`${method} failed: invalid JSON-RPC response`);
  }
  const body = json as { error?: unknown; result?: unknown };
  if (body.error) {
    throw new Error(formatRpcError(body.error, method));
  }
  return body.result;
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
    return err.message.trim() || "Request failed";
  }
  if (typeof err === "string") {
    return err.trim() || "Request failed";
  }
  if (err && typeof err === "object" && "message" in err) {
    const msg = String((err as { message: unknown }).message).trim();
    return msg || "Request failed";
  }
  return "Request failed";
}
