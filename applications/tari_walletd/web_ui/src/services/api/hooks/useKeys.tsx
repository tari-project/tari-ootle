//  Copyright 2022. The Tari Project
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

import { ApiError } from "@api/helpers/types";
import queryClient from "@api/queryClient";
import { useMutation, useQuery } from "@tanstack/react-query";
import { KeyBranch } from "@tari-project/ootle-ts-bindings";
import { keysCreate, keysList, keysSetActive } from "@utils/json_rpc";

export const useKeysList = (branch: KeyBranch) => {
  return useQuery({
    queryKey: ["keys_list", branch],
    queryFn: () => {
      return keysList({ branch });
    },
  });
};

export const useKeysCreate = (branch: KeyBranch) => {
  return useMutation({
    mutationFn: () => keysCreate({ branch, specific_index: null }),
    onError: (error: ApiError) => {
      error;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["keys_list"] });
    },
  });
};

export const useKeysSetActive = () => {
  const setActive = async (index: bigint) => {
    const result = await keysSetActive({ index: Number(index) });
    return result;
  };

  return useMutation({
    mutationFn: setActive,
    onError: (error: ApiError) => {
      error;
    },
    onSuccess: () => {
      queryClient.invalidateQueries({ queryKey: ["keys_list"] });
    },
  });
};
