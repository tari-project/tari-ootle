const test = require("node:test");
const assert = require("node:assert/strict");
const fs = require("node:fs");
const path = require("node:path");
const ts = require("typescript");

const moduleCache = new Map();

function loadTs(relPath, baseDir = __dirname) {
  let fullPath = path.resolve(baseDir, relPath);
  if (!fullPath.endsWith(".ts")) {
    if (fs.existsSync(fullPath + ".ts")) fullPath += ".ts";
    else if (fs.existsSync(path.join(fullPath, "index.ts"))) fullPath = path.join(fullPath, "index.ts");
  }
  if (moduleCache.has(fullPath)) return moduleCache.get(fullPath);

  const code = fs.readFileSync(fullPath, "utf8");
  const result = ts.transpileModule(code, {
    compilerOptions: {
      module: ts.ModuleKind.CommonJS,
      target: ts.ScriptTarget.ES2022,
      esModuleInterop: true,
    },
  });
  const m = { exports: {} };
  moduleCache.set(fullPath, m.exports);

  const localRequire = (reqPath) => {
    if (reqPath.startsWith(".")) {
      return loadTs(reqPath, path.dirname(fullPath));
    }
    return require(reqPath);
  };

  const fn = new Function("require", "module", "exports", "__filename", "__dirname", result.outputText);
  fn(localRequire, m, m.exports, fullPath, path.dirname(fullPath));
  moduleCache.set(fullPath, m.exports);
  return m.exports;
}

const { FetchRpcTransport } = loadTs("../src/transports/index.ts");
const { WalletDaemonClient } = loadTs("../src/index.ts");

test("FetchRpcTransport: clears timeoutId in finally block on success", async () => {
  const origFetch = globalThis.fetch;
  let cleared = false;
  const origClearTimeout = globalThis.clearTimeout;
  globalThis.clearTimeout = (id) => {
    cleared = true;
    return origClearTimeout(id);
  };

  globalThis.fetch = async () => ({
    ok: true,
    json: async () => ({ id: 1, jsonrpc: "2.0", result: { status: "ok" } }),
  });

  try {
    const transport = new FetchRpcTransport("http://localhost:18200");
    const res = await transport.sendRequest(
      { id: 1, jsonrpc: "2.0", method: "test", params: {} },
      { timeout_millis: 5000 },
    );
    assert.deepEqual(res.result, { status: "ok" });
    assert.equal(cleared, true);
  } finally {
    globalThis.fetch = origFetch;
    globalThis.clearTimeout = origClearTimeout;
  }
});

test("FetchRpcTransport: clears timeoutId in finally block on fetch error/rejection", async () => {
  const origFetch = globalThis.fetch;
  let cleared = false;
  const origClearTimeout = globalThis.clearTimeout;
  globalThis.clearTimeout = (id) => {
    cleared = true;
    return origClearTimeout(id);
  };

  globalThis.fetch = async () => {
    throw new TypeError("Failed to fetch");
  };

  try {
    const transport = new FetchRpcTransport("http://localhost:18200");
    await assert.rejects(
      transport.sendRequest(
        { id: 1, jsonrpc: "2.0", method: "test", params: {} },
        { timeout_millis: 5000 },
      ),
      TypeError,
    );
    assert.equal(cleared, true);
  } finally {
    globalThis.fetch = origFetch;
    globalThis.clearTimeout = origClearTimeout;
  }
});

test("WalletDaemonClient: deduplicates concurrent 401 refresh requests", async () => {
  let refreshCalls = 0;
  let attempts = 0;

  const fakeTransport = {
    sendRequest: async (req) => {
      if (req.method === "auth.refresh") {
        refreshCalls++;
        // Simulate a slight delay in refresh
        await new Promise((r) => setTimeout(r, 20));
        return {
          id: req.id,
          jsonrpc: "2.0",
          result: { token: "refreshed_jwt_token" },
        };
      }

      attempts++;
      // First 2 calls return 401 unauthorized
      if (attempts <= 2) {
        return {
          id: req.id,
          jsonrpc: "2.0",
          error: { code: 401, message: "Unauthorized" },
        };
      }

      // Subsequent retries succeed
      return {
        id: req.id,
        jsonrpc: "2.0",
        result: { data: "success_" + req.params.val },
      };
    },
  };

  const client = new WalletDaemonClient(fakeTransport);
  client.setToken("expired_jwt_token");

  // Launch two concurrent requests that both encounter 401
  const [res1, res2] = await Promise.all([
    client.sendRequest("query.one", { val: 1 }),
    client.sendRequest("query.two", { val: 2 }),
  ]);

  assert.equal(res1.data, "success_1");
  assert.equal(res2.data, "success_2");
  // Only 1 auth.refresh call should have been made, not 2
  assert.equal(refreshCalls, 1);
  assert.equal(client.getToken(), "refreshed_jwt_token");
});

test("WalletDaemonClient: clears token and throws origError when refresh fails", async () => {
  const fakeTransport = {
    sendRequest: async (req) => {
      if (req.method === "auth.refresh") {
        return {
          id: req.id,
          jsonrpc: "2.0",
          error: { code: 401, message: "Refresh token expired" },
        };
      }
      return {
        id: req.id,
        jsonrpc: "2.0",
        error: { code: 401, message: "Session expired" },
      };
    },
  };

  const client = new WalletDaemonClient(fakeTransport);
  client.setToken("expired_token");

  await assert.rejects(
    client.sendRequest("query.test", {}),
    /RPC Error 401: Session expired/,
  );

  assert.equal(client.getToken(), null);
});
