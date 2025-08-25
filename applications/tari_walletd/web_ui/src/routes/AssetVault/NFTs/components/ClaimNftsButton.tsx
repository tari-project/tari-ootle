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
import { useMintTestnetFaucetNfts } from "../../../../api/hooks/useAccounts";
import useAccountStore from "../../../../store/accountStore";
import { substateIdToString } from "@tari-project/typescript-bindings";

function ClaimNftsButton() {
  const { mutate: claimTestnetFaucetNfts } = useMintTestnetFaucetNfts();
  const account = useAccountStore((state) => state.account);

  if (!account) {
    return <></>;
  }

  const onClaimTestnetNfts = () => {
    claimTestnetFaucetNfts(
      {
        account: { ComponentAddress: substateIdToString(account.address) },
        numberToMint: 5,
        mutableData: {
          image_url: "https://img.freepik.com/free-vector/gradient-isometric-nft-concept_52683-62009.jpg?w=740",
        },
        maxFee: 2000,
      },
      {
        onSuccess: (resp) => {
          console.log(resp);
        },
      },
    );
  };

  return (
    <Button variant="outlined" onClick={() => onClaimTestnetNfts()}>
      Claim Testnet NFTs
    </Button>
  );
}

export default ClaimNftsButton;
