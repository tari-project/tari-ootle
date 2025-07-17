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
import { GeneratedCodeType } from "@tari-project/tari-extension-common";
import { AccountInfo } from "@tari-project/typescript-bindings";
import { persist } from "zustand/middleware";

interface FlowEditorStore {
  panelOpen: boolean;
  setPanelOpen: (open: boolean) => void;
  templateId: string;
  setTemplateId: (id: string) => void;
  codeDialogOpen: boolean;
  setCodeDialogOpen: (open: boolean) => void;
  generatedCode: string;
  setGeneratedCode: (code: string) => void;
  generatedCodeType: GeneratedCodeType | null;
  setGeneratedCodeType: (type: GeneratedCodeType | null) => void;
  account: AccountInfo | undefined;
  setAccount: (account: AccountInfo | undefined) => void;
  setCurrentState: (state: any) => void;
  currentState: any;
  fee: number;
  setFee: (fee: number) => void;
}

export const INITIAL_FLOW_STATE = {
  $schema: "https://tari-project.github.io/tari-vscode-nocode-extension/schemas/tari-schema-v1.0.json",
  version: "1.0",
  nodes: [],
  edges: [],
};

const useFlowEditorStore = create<FlowEditorStore>()(
  persist<FlowEditorStore>(
    (set) => ({
      panelOpen: true,
      setPanelOpen: (open) => set({ panelOpen: open }),
      templateId: "",
      setTemplateId: (id) => set({ templateId: id }),
      codeDialogOpen: false,
      setCodeDialogOpen: (open) => set({ codeDialogOpen: open }),
      generatedCode: "",
      setGeneratedCode: (code) => set({ generatedCode: code }),
      generatedCodeType: null,
      setGeneratedCodeType: (type) => set({ generatedCodeType: type }),
      account: undefined,
      setAccount: (account) => set({ account }),
      currentState: INITIAL_FLOW_STATE,
      setCurrentState: (json) => set({ currentState: json }),
      fee: 3000,
      setFee: (fee) => set({ fee }),
    }),
    { name: "flowEditor" },
  ),
);

export default useFlowEditorStore;
