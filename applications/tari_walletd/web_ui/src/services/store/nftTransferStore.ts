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

import type { NonFungibleId, OotleAddress, ResourceAddress } from "@tari-project/ootle-ts-bindings";
import { create } from "zustand";

export type DialogStep = "form" | "confirmation" | "result";

interface TransferFormState {
  payerAccount: string;
  nfts: NonFungibleId[];
  targetAccountAddress: OotleAddress;
  maxFee: string;
  resourceAddress: ResourceAddress;
}

interface TransferResult {
  success: boolean;
  message: string;
}

interface Validity {
  payerAccount: boolean;
  nfts: boolean;
  targetAccountPublicKey: boolean;
}

interface NftTransferState {
  // Dialog state
  currentStep: DialogStep;
  disabled: boolean;

  // Form state
  transferFormState: TransferFormState;
  validity: Validity;

  // Fee estimation state
  estimatedFee: number | null;
  isEstimatingFee: boolean;

  // Result state
  transferResult: TransferResult | null;

  // Auto-close state
  autoCloseTimeoutId: NodeJS.Timeout | null;

  // Actions
  setCurrentStep: (step: DialogStep) => void;
  setDisabled: (disabled: boolean) => void;
  setTransferFormState: (state: Partial<TransferFormState>) => void;
  setValidity: (validity: Partial<Validity>) => void;
  setEstimatedFee: (fee: number | null) => void;
  setIsEstimatingFee: (estimating: boolean) => void;
  setTransferResult: (result: TransferResult | null) => void;
  setAutoCloseTimeoutId: (timeoutId: NodeJS.Timeout | null) => void;

  // Complex actions
  updateFormValue: (name: string, value: string, isValid?: boolean) => void;
  initializeFormState: (
    preSelectedNftId?: NonFungibleId,
    preSelectedResourceAddress?: ResourceAddress,
    accountAddress?: string,
  ) => void;
  resetState: (preSelectedNftId?: NonFungibleId, preSelectedResourceAddress?: ResourceAddress, accountAddress?: string) => void;
  isFormValid: () => boolean;
}

const createInitialFormState = (
  preSelectedNftId?: NonFungibleId,
  preSelectedResourceAddress?: ResourceAddress,
  accountAddress?: string,
): TransferFormState => ({
  payerAccount: accountAddress || "",
  nfts: preSelectedNftId ? [preSelectedNftId] : [],
  targetAccountAddress: "",
  maxFee: "",
  resourceAddress: (preSelectedResourceAddress || "") as ResourceAddress,
});

const createInitialValidity = (preSelectedNftId?: NonFungibleId): Validity => ({
  payerAccount: true,
  nfts: preSelectedNftId ? true : false,
  targetAccountPublicKey: false,
});

export const useNftTransferStore = create<NftTransferState>((set, get) => ({
  // Initial state
  currentStep: "form",
  disabled: false,
  transferFormState: createInitialFormState(),
  validity: createInitialValidity(),
  estimatedFee: null,
  isEstimatingFee: false,
  transferResult: null,
  autoCloseTimeoutId: null,

  // Simple setters
  setCurrentStep: (step) => set({ currentStep: step }),
  setDisabled: (disabled) => set({ disabled }),
  setTransferFormState: (state) =>
    set((prev) => ({
      transferFormState: { ...prev.transferFormState, ...state },
    })),
  setValidity: (validity) =>
    set((prev) => ({
      validity: { ...prev.validity, ...validity },
    })),
  setEstimatedFee: (fee) => set({ estimatedFee: fee }),
  setIsEstimatingFee: (estimating) => set({ isEstimatingFee: estimating }),
  setTransferResult: (result) => set({ transferResult: result }),
  setAutoCloseTimeoutId: (timeoutId) => set({ autoCloseTimeoutId: timeoutId }),

  // Complex actions
  updateFormValue: (name, value, isValid) => {
    const { transferFormState, validity } = get();

    set({
      transferFormState: { ...transferFormState, [name]: value },
      validity: isValid !== undefined ? { ...validity, [name]: isValid } : validity,
    });
  },

  initializeFormState: (preSelectedNftId, preSelectedResourceAddress, accountAddress) => {
    set({
      transferFormState: createInitialFormState(preSelectedNftId, preSelectedResourceAddress, accountAddress),
      validity: createInitialValidity(preSelectedNftId),
    });
  },

  resetState: (preSelectedNftId, preSelectedResourceAddress, accountAddress) => {
    const { autoCloseTimeoutId } = get();

    // Clear any active timeout
    if (autoCloseTimeoutId) {
      clearTimeout(autoCloseTimeoutId);
    }

    set({
      currentStep: "form",
      disabled: false,
      transferFormState: createInitialFormState(preSelectedNftId, preSelectedResourceAddress, accountAddress),
      validity: createInitialValidity(preSelectedNftId),
      estimatedFee: null,
      isEstimatingFee: false,
      transferResult: null,
      autoCloseTimeoutId: null,
    });
  },

  isFormValid: () => {
    const { validity } = get();
    return Object.values(validity).every((v) => v);
  },
}));
