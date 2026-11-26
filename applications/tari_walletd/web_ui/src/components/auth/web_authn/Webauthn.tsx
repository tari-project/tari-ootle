// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useWebauthnAlreadyRegistered } from "@api/hooks/useWebauthn";
import Error from "@components/Error";
import Loading from "@components/Loading";
import type { Permission } from "@tari-project/ootle-ts-bindings";
import { useEffect } from "react";
import WebauthnLogin from "./components/Login";
import WebauthnRegistration from "./components/Registration";
import { resolveWebauthnView } from "./webauthnHelpers";

export const APP_NAME: string = "tari-wallet-webui";
export const DEFAULT_PERMISSIONS: Permission[] = ["Admin"];

export interface WebauthnProps {
  onAuthenticated: () => void;
}

export default function WebAuthn(props: WebauthnProps) {
  const { onAuthenticated } = props;
  const {
    data: alreadyRegisteredResponse,
    isLoading: alreadyRegisteredIsLoading,
    isError: alreadyRegisteredIsError,
    error: alreadyRegisteredError,
  } = useWebauthnAlreadyRegistered();

  useEffect(() => {
    if (alreadyRegisteredIsError) {
      console.error(alreadyRegisteredError);
    }
  }, [alreadyRegisteredIsError, alreadyRegisteredError]);

  const view = resolveWebauthnView({
    isLoading: alreadyRegisteredIsLoading,
    isError: alreadyRegisteredIsError,
    errorMessage: alreadyRegisteredError?.message,
    registered: alreadyRegisteredResponse?.registered,
  });

  switch (view.kind) {
    case "loading":
      return <Loading />;
    case "error":
      return <Error message={view.message} />;
    case "register":
      return <WebauthnRegistration onAuthenticated={onAuthenticated} />;
    case "login":
      return <WebauthnLogin onAuthenticated={onAuthenticated} />;
  }
}
