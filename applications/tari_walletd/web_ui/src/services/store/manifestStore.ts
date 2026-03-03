//  Copyright 2025. The Tari Project
//
//  Redistribution and use in source and binary forms, with or without modification, are permitted provided that the
//  following conditions are met:
//
//  1. Redistributions of source code must retain the above copyright notice, this list of conditions and the following
//  disclaimer.
//
//  2. Redistributions in binary form must reproduce the above copyright notice, this list of conditions and the
//  following disclaimer in the documentation and/or other materials provided with the distribution.
//
//  3. Neither the name of the copyright holder nor the names of its contributors may be used to endorse or promote
//  products derived from this software without specific prior written permission.
//
//  THIS SOFTWARE IS PROVIDED BY THE COPYRIGHT HOLDERS AND CONTRIBUTORS "AS IS" AND ANY EXPRESS OR IMPLIED WARRANTIES,
//  INCLUDING, BUT NOT LIMITED TO, THE IMPLIED WARRANTIES OF MERCHANTABILITY AND FITNESS FOR A PARTICULAR PURPOSE ARE
//  DISCLAIMED. IN NO EVENT SHALL THE COPYRIGHT HOLDER OR CONTRIBUTORS BE LIABLE FOR ANY DIRECT, INDIRECT, INCIDENTAL,
//  SPECIAL, EXEMPLARY, OR CONSEQUENTIAL DAMAGES (INCLUDING, BUT NOT LIMITED TO, PROCUREMENT OF SUBSTITUTE GOODS OR
//  SERVICES; LOSS OF USE, DATA, OR PROFITS; OR BUSINESS INTERRUPTION) HOWEVER CAUSED AND ON ANY THEORY OF LIABILITY,
//  WHETHER IN CONTRACT, STRICT LIABILITY, OR TORT (INCLUDING NEGLIGENCE OR OTHERWISE) ARISING IN ANY WAY OUT OF THE
//  USE OF THIS SOFTWARE, EVEN IF ADVISED OF THE POSSIBILITY OF SUCH DAMAGE.

import { create } from "zustand";
import { persist } from "zustand/middleware";

const DEFAULT_CODE = `
// use template_xxx as TemplateName;

fn main() {
   // TemplateName::call_something();
   // let account = var!["account"];
   // let bucket = account.withdraw(1000);
}`;

export interface ManifestTab {
  id: string;
  name: string;
  code: string;
  variables: Record<string, string>;
}

interface Store {
  tabs: ManifestTab[];
  activeTabId: string;

  // Active tab convenience accessors
  code: string;
  variables: Record<string, string>;

  // Tab management
  addTab: () => void;
  removeTab: (id: string) => void;
  setActiveTab: (id: string) => void;
  renameTab: (id: string, name: string) => void;

  // Active tab code/variable operations
  setCode: (code: string) => void;
  addVariable: (key: string, value: string) => void;
  removeVariable: (key: string) => void;
  renameVariable: (oldKey: string, newKey: string) => void;
}

let nextId = 1;
function generateId(): string {
  return `tab_${Date.now()}_${nextId++}`;
}

function createTab(name: string): ManifestTab {
  return { id: generateId(), name, code: DEFAULT_CODE, variables: {} };
}

function nextTabName(tabs: ManifestTab[]): string {
  let i = 1;
  const names = new Set(tabs.map((t) => t.name));
  while (names.has(`Manifest ${i}`)) i++;
  return `Manifest ${i}`;
}

function updateActiveTab(state: Store, patch: Partial<ManifestTab>): Partial<Store> {
  const tabs = state.tabs.map((t) => (t.id === state.activeTabId ? { ...t, ...patch } : t));
  const active = tabs.find((t) => t.id === state.activeTabId)!;
  return { tabs, code: active.code, variables: active.variables };
}

const defaultTab = createTab("Manifest 1");

const useManifestCodeStore = create<Store>()(
  persist<Store>(
    (set) => ({
      tabs: [defaultTab],
      activeTabId: defaultTab.id,
      code: defaultTab.code,
      variables: defaultTab.variables,

      addTab: () =>
        set((state) => {
          const tab = createTab(nextTabName(state.tabs));
          return {
            tabs: [...state.tabs, tab],
            activeTabId: tab.id,
            code: tab.code,
            variables: tab.variables,
          };
        }),

      removeTab: (id: string) =>
        set((state) => {
          if (state.tabs.length <= 1) return state;
          const idx = state.tabs.findIndex((t) => t.id === id);
          const tabs = state.tabs.filter((t) => t.id !== id);
          let activeTabId = state.activeTabId;
          if (activeTabId === id) {
            const newIdx = Math.min(idx, tabs.length - 1);
            activeTabId = tabs[newIdx].id;
          }
          const active = tabs.find((t) => t.id === activeTabId)!;
          return { tabs, activeTabId, code: active.code, variables: active.variables };
        }),

      setActiveTab: (id: string) =>
        set((state) => {
          const tab = state.tabs.find((t) => t.id === id);
          if (!tab) return state;
          return { activeTabId: id, code: tab.code, variables: tab.variables };
        }),

      renameTab: (id: string, name: string) =>
        set((state) => ({
          tabs: state.tabs.map((t) => (t.id === id ? { ...t, name } : t)),
        })),

      setCode: (code: string) => set((state) => updateActiveTab(state, { code })),

      addVariable: (key: string, value: string) =>
        set((state) => {
          const active = state.tabs.find((t) => t.id === state.activeTabId)!;
          return updateActiveTab(state, { variables: { ...active.variables, [key]: value } });
        }),

      removeVariable: (key: string) =>
        set((state) => {
          const active = state.tabs.find((t) => t.id === state.activeTabId)!;
          const { [key]: _, ...rest } = active.variables;
          return updateActiveTab(state, { variables: rest });
        }),

      renameVariable: (oldKey: string, newKey: string) =>
        set((state) => {
          const active = state.tabs.find((t) => t.id === state.activeTabId)!;
          if (oldKey === newKey || !(oldKey in active.variables)) return state;
          const entries = Object.entries(active.variables).map(([k, v]) => (k === oldKey ? [newKey, v] : [k, v]));
          return updateActiveTab(state, { variables: Object.fromEntries(entries) });
        }),
    }),
    {
      name: "manifest-code",
      migrate: (persisted: unknown, version: number) => {
        const state = persisted as Record<string, unknown>;
        // Migrate from single-tab format (no tabs array) to multi-tab format
        if (version === 0 && state && !state.tabs) {
          const code = (state.code as string) || DEFAULT_CODE;
          const variables = (state.variables as Record<string, string>) || {};
          const tab: ManifestTab = { id: generateId(), name: "Manifest 1", code, variables };
          return {
            ...state,
            tabs: [tab],
            activeTabId: tab.id,
            code: tab.code,
            variables: tab.variables,
          } as Store;
        }
        return state as unknown as Store;
      },
      version: 1,
    },
  ),
);

export default useManifestCodeStore;
