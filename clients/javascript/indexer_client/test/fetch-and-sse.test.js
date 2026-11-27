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

const { FetchTransport } = loadTs("../src/transports/index.ts");
const { connectSse } = loadTs("../src/sse.ts");
const { IndexerClient } = loadTs("../src/index.ts");

test("FetchTransport: strips trailing slash in constructor and static new", () => {
  const t1 = new FetchTransport("http://localhost:18300/");
  assert.equal(t1.url, "http://localhost:18300");

  const t2 = FetchTransport.new("http://localhost:18300/");
  assert.equal(t2.url, "http://localhost:18300");

  const t3 = new FetchTransport("http://localhost:18300");
  assert.equal(t3.url, "http://localhost:18300");
});

test("FetchTransport: clears timeoutId in finally block on successful request", async () => {
  const origFetch = globalThis.fetch;
  let cleared = false;
  const origClearTimeout = globalThis.clearTimeout;
  globalThis.clearTimeout = (id) => {
    cleared = true;
    return origClearTimeout(id);
  };

  globalThis.fetch = async () => ({
    ok: true,
    json: async () => ({ result: "ok" }),
  });

  try {
    const transport = new FetchTransport("http://localhost:18300");
    const res = await transport.sendRequest("test", { method: "GET" }, { timeout_millis: 5000 });
    assert.deepEqual(res, { result: "ok" });
    assert.equal(cleared, true);
  } finally {
    globalThis.fetch = origFetch;
    globalThis.clearTimeout = origClearTimeout;
  }
});

test("FetchTransport: clears timeoutId in finally block on fetch error/rejection", async () => {
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
    const transport = new FetchTransport("http://localhost:18300");
    await assert.rejects(
      transport.sendRequest("test", { method: "GET" }, { timeout_millis: 5000 }),
      TypeError,
    );
    assert.equal(cleared, true);
  } finally {
    globalThis.fetch = origFetch;
    globalThis.clearTimeout = origClearTimeout;
  }
});

test("connectSse: removes external signal abort listener when stream closes", async () => {
  const origFetch = globalThis.fetch;
  const externalAc = new AbortController();

  // Create a readable stream that delivers one event and closes
  const encoder = new TextEncoder();
  const stream = new ReadableStream({
    start(controller) {
      controller.enqueue(encoder.encode("event: test\ndata: hello\n\n"));
      controller.close();
    },
  });

  globalThis.fetch = async () => ({
    ok: true,
    status: 200,
    statusText: "OK",
    body: stream,
  });

  try {
    let closed = false;
    let eventReceived = false;

    await new Promise((resolve) => {
      connectSse("http://localhost:18300/events", {
        signal: externalAc.signal,
        onEvent: (event) => {
          if (event.event === "test") eventReceived = true;
        },
        onClose: () => {
          closed = true;
          resolve();
        },
      });
    });

    assert.equal(eventReceived, true);
    assert.equal(closed, true);

    // After closing, triggering externalAc should not fire any leftover handler or throw
    assert.doesNotThrow(() => {
      externalAc.abort();
    });
  } finally {
    globalThis.fetch = origFetch;
  }
});

test("IndexerClient.getTransactionReceipt: URL-encodes address with special characters", async () => {
  let calledUrl = null;
  const fakeTransport = {
    sendGet: async (path) => {
      calledUrl = path;
      return { receipt: {} };
    },
  };

  const client = new IndexerClient(fakeTransport);
  const testAddress = "substate/address:with+special=chars";
  await client.getTransactionReceipt(testAddress);

  assert.equal(calledUrl, `transaction-receipts/${encodeURIComponent(testAddress)}`);
});
