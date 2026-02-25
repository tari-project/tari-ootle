// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useEffect, useState } from "react";
import WebauthnLogin from "./components/Login";
import WebauthnRegistration from "./components/Registration";
import { useWebauthnAlreadyRegistered } from "@api/hooks/useWebauthn";
import Loading from "@components/Loading";
import { JrpcPermission } from "@tari-project/ootle-ts-bindings";

export const APP_NAME: string = "tari-wallet-webui";
export const DEFAULT_PERMISSIONS: JrpcPermission[] = ["Admin"];

export interface WebauthnProps {
  onAuthenticated: () => void;
}

export default function WebAuthn(props: WebauthnProps) {
  const { onAuthenticated } = props;
  const [registered, setRegistered] = useState(false);
  const {
    data: alreadyRegisteredResponse,
    isLoading: alreadyRegisteredIsLoading,
    isError: alreadyRegisteredIsError,
    error: alreadyRegisteredError,
  } = useWebauthnAlreadyRegistered(APP_NAME);

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
    return <WebauthnRegistration onAuthenticated={onAuthenticated} />;
  }

  return <WebauthnLogin onAuthenticated={onAuthenticated} />;
}
