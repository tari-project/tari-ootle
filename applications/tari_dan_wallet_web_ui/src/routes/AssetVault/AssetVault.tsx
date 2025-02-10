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

import {useAccountsGetDefault} from "../../api/hooks/useAccounts";
import useAccountStore from "../../store/accountStore";
import Onboarding from "../Onboarding/Onboarding";
import MyAssets from "./Components/MyAssets";
import {useEffect, useState} from "react";
import FetchStatusCheck from "../../Components/FetchStatusCheck";
import {useAuthMethod} from "../../api/hooks/useAuth";
import Loading from "../../Components/Loading";
import WebauthnRegistration from "../WebauthnRegistration/WebauthnRegistration";

function AssetVault() {
  const { account, setAccount, setPublicKey } = useAccountStore();
  const { data: defaultAccount, isLoading, isError, error } = useAccountsGetDefault();
  const { data: authMethod } = useAuthMethod();
  const [ currAuthMethod, setCurrAuthMethod ] = useState('');

  useEffect(() => {
    if (!isError && defaultAccount) {
      setAccount(defaultAccount.account);
      setPublicKey(defaultAccount.public_key);
    }

    if (error) {
      console.info(error);
    }

    if (authMethod) {
      setCurrAuthMethod(authMethod.method);
    }
  }, [defaultAccount, isError, authMethod, setCurrAuthMethod]);

  const onboarding = (() => {
    switch(currAuthMethod) {
      case 'none': {
        return <Onboarding />;
      }
      case 'webauthn': {
        return <WebauthnRegistration />;
      }
      default: {
        return <Loading />;
      }
    }
  });

  return (
    <FetchStatusCheck errorMessage={""} isError={false} isLoading={isLoading}>
      {account ? <MyAssets /> : onboarding()}
    </FetchStatusCheck>
  );
}

export default AssetVault;
