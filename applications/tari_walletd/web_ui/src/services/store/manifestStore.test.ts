//  Copyright 2026 The Tari Project
//  SPDX-License-Identifier: BSD-3-Clause

import type { KeyId } from "@tari-project/ootle-ts-bindings";
import { beforeEach, describe, expect, it } from "vitest";
import useManifestCodeStore, { isSameKeyId, type ManifestTab, normalizeStringRecord } from "./manifestStore";

describe("manifestStore", () => {
  beforeEach(() => {
    useManifestCodeStore.getState().loadTabs([
      {
        id: "seed",
        name: "Manifest 1",
        code: "fn main() {}",
        variables: {},
        signingKeys: [],
        blobs: {},
      },
    ]);
  });

  it("refuses to overwrite an existing variable when renaming to a duplicate key", () => {
    const store = useManifestCodeStore.getState();
    store.addVariable("account_1", "component_11");
    store.addVariable("account_2", "component_22");

    expect(useManifestCodeStore.getState().renameVariable("account_1", "account_2")).toBe(false);
    expect(useManifestCodeStore.getState().variables).toEqual({
      account_1: "component_11",
      account_2: "component_22",
    });

    expect(useManifestCodeStore.getState().renameVariable("account_2", "account_1")).toBe(false);
    expect(useManifestCodeStore.getState().variables).toEqual({
      account_1: "component_11",
      account_2: "component_22",
    });
  });

  it("renames a variable in place and ignores prototype keys", () => {
    const store = useManifestCodeStore.getState();
    store.addVariable("first", "v1");
    store.addVariable("second", "v2");

    expect(useManifestCodeStore.getState().renameVariable("first", "renamed")).toBe(true);
    expect(Object.entries(useManifestCodeStore.getState().variables)).toEqual([
      ["renamed", "v1"],
      ["second", "v2"],
    ]);

    expect(useManifestCodeStore.getState().renameVariable("toString", "other")).toBe(false);
    expect(useManifestCodeStore.getState().variables).toEqual({
      renamed: "v1",
      second: "v2",
    });
  });

  it("deduplicates signing keys structurally regardless of property order", () => {
    const keyNum: KeyId = { Derived: { key_branch: "account", index: 0 } };
    const keyReordered: KeyId = { Derived: { index: 0, key_branch: "account" } };
    const keyOther: KeyId = { Derived: { key_branch: "account", index: 1 } };

    useManifestCodeStore.getState().addSigningKey(keyNum);
    useManifestCodeStore.getState().addSigningKey(keyReordered);
    useManifestCodeStore.getState().addSigningKey(keyOther);

    expect(useManifestCodeStore.getState().signingKeys).toEqual([keyNum, keyOther]);
  });

  it("normalizes missing or invalid variables, signingKeys, and blobs in loadTabs", () => {
    const partialTabs = [
      {
        name: "Minimal",
        code: "fn main() {}",
      },
      {
        name: "Malformed fields",
        code: "fn second() {}",
        variables: ["bad"],
        signingKeys: "bad",
        blobs: null,
      },
    ] as unknown as ManifestTab[];

    useManifestCodeStore.getState().loadTabs(partialTabs);
    const state = useManifestCodeStore.getState();

    expect(state.tabs).toHaveLength(2);
    expect(state.variables).toEqual({});
    expect(state.signingKeys).toEqual([]);
    expect(state.blobs).toEqual({});
    expect(state.tabs[1].variables).toEqual({});
    expect(state.tabs[1].signingKeys).toEqual([]);
    expect(state.tabs[1].blobs).toEqual({});
  });
});

describe("manifestStore helpers", () => {
  it("compares Derived and Imported KeyIds structurally across numeric representations", () => {
    expect(
      isSameKeyId({ Derived: { key_branch: "account", index: 2 } }, { Derived: { index: 2n, key_branch: "account" } }),
    ).toBe(true);
    expect(
      isSameKeyId(
        { Derived: { key_branch: "account", index: 2 } },
        { Derived: { key_branch: "view_only_key", index: 2 } },
      ),
    ).toBe(false);
    expect(isSameKeyId({ Imported: { local_key_id: 7 } }, { Imported: { local_key_id: 7n } })).toBe(true);
    expect(isSameKeyId({ Imported: { local_key_id: 0 } }, { Derived: { key_branch: "account", index: 0 } })).toBe(
      false,
    );
  });

  it("filters non-string entries in normalizeStringRecord", () => {
    expect(normalizeStringRecord(undefined)).toEqual({});
    expect(normalizeStringRecord(null)).toEqual({});
    expect(normalizeStringRecord(["a"])).toEqual({});
    expect(normalizeStringRecord({ good: "ok", bad: 123, alsoBad: null })).toEqual({ good: "ok" });
  });
});
