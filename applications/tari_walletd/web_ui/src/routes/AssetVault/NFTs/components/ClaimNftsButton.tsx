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

import Button from "@mui/material/Button";
import { useMintTestnetFaucetNfts } from "@api/hooks/useAccounts";
import useAccountStore from "@store/accountStore";
import { substateIdToString } from "@tari-project/ootle-ts-bindings";
import queryClient from "@api/queryClient";
import { useErrorNotification } from "../../../../contexts/ErrorNotificationContext";

function ClaimNftsButton() {
  const { mutate: claimTestnetFaucetNfts, isPending } = useMintTestnetFaucetNfts();
  const account = useAccountStore((state) => state.account);
  const { showError, showSuccess } = useErrorNotification();

  if (!account) {
    return <></>;
  }

  const onClaimTestnetNfts = () => {
    claimTestnetFaucetNfts(
      {
        account: { ComponentAddress: substateIdToString(account.component_address) },
        numberToMint: 5,
        mutableData: {
          image_url: "https://img.freepik.com/free-vector/gradient-isometric-nft-concept_52683-62009.jpg?w=740",
        },
        maxFee: 2000,
      },
      {
        onSuccess: (resp: any) => {
          console.log(resp);
          showSuccess("Successfully claimed NFTs!");
          // Invalidate NFT queries to refresh the list
          queryClient.invalidateQueries({
            predicate: (query) => {
              const key = query.queryKey[0];
              return typeof key === "string" && (key === "nfts" || key === "list_nfts" || key === "nfts_list");
            },
          });
        },
        onError: (error: any) => {
          console.error("Error claiming NFTs:", error);
          // Show user-friendly error message
          const errorMessage =
            error?.message ||
            "Failed to claim NFTs. Please ensure you have sufficient funds to pay for transaction fees.";
          showError(errorMessage);
        },
      },
    );
  };

  return (
    <Button variant="outlined" onClick={() => onClaimTestnetNfts()} disabled={isPending}>
      {isPending ? "Claiming..." : "Claim Testnet NFTs"}
    </Button>
  );
}

export default ClaimNftsButton;
