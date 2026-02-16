// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useEffect, useState } from "react";
import WebauthnLogin from "./Components/Login";
import WebauthnRegistration from "./Components/Registration";
import { useWebauthnAlreadyRegistered } from "@api/hooks/useWebauthn";
import Loading from "@components/Loading";
import useAuthStore from "@store/authStore";
import { useSearchParams } from "react-router-dom";
import { JrpcPermission } from "@tari-project/ootle-ts-bindings";

export const APP_NAME: string = "tari-wallet-webui";
export const DEFAULT_PERMISSIONS: JrpcPermission[] = ["Admin"];
function Webauthn() {
  const [registered, setRegistered] = useState(false);
  const {
    data: alreadyRegisteredResponse,
    isLoading: alreadyRegisteredIsLoading,
    isError: alreadyRegisteredIsError,
    error: alreadyRegisteredError,
  } = useWebauthnAlreadyRegistered(APP_NAME);
  const [searchParams] = useSearchParams();
  const redirectQuery = searchParams.get("redirect");
  const redirect = redirectQuery ? redirectQuery : "/";

  useEffect(() => {
    if (!alreadyRegisteredIsError && alreadyRegisteredResponse) {
      setRegistered(alreadyRegisteredResponse.registered);
    }

    if (alreadyRegisteredIsError) {
      console.error(alreadyRegisteredError);
    }
  }, [alreadyRegisteredResponse, alreadyRegisteredIsError]);

  if (alreadyRegisteredIsLoading) {
    return <Loading />;
  }

  if (!registered) {
    return <WebauthnRegistration redirect={redirect} />;
  }

  return <WebauthnLogin redirect={redirect} />;
}

export default Webauthn;
