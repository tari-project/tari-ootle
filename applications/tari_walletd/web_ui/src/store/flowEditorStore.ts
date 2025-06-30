// Copyright 2022. The Tari Project
//
// Store for FlowEditor state using zustand

import { create } from "zustand";
import { GeneratedCodeType } from "@tari-project/tari-extension-common";

interface FlowEditorStore {
  panelOpen: boolean;
  setPanelOpen: (open: boolean) => void;
  componentId: string;
  setComponentId: (id: string) => void;
  codeDialogOpen: boolean;
  setCodeDialogOpen: (open: boolean) => void;
  generatedCode: string;
  setGeneratedCode: (code: string) => void;
  generatedCodeType: GeneratedCodeType | null;
  setGeneratedCodeType: (type: GeneratedCodeType | null) => void;
  reset: () => void;
}

const useFlowEditorStore = create<FlowEditorStore>()((set) => ({
  panelOpen: true,
  setPanelOpen: (open) => set({ panelOpen: open }),
  componentId: "",
  setComponentId: (id) => set({ componentId: id }),
  codeDialogOpen: false,
  setCodeDialogOpen: (open) => set({ codeDialogOpen: open }),
  generatedCode: "",
  setGeneratedCode: (code) => set({ generatedCode: code }),
  generatedCodeType: null,
  setGeneratedCodeType: (type) => set({ generatedCodeType: type }),
  reset: () => set({
    panelOpen: true,
    componentId: "",
    codeDialogOpen: false,
    generatedCode: "",
    generatedCodeType: null,
  }),
}));

export default useFlowEditorStore;
