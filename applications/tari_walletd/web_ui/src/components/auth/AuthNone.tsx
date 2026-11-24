//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import Loading from "@components/Loading";
import { Typography } from "@mui/material";
import { getClientInstance } from "@utils/json_rpc";
import { useEffect, useRef, useState } from "react";
import { runAuthNone } from "./authNoneHelpers";
import { DEFAULT_PERMISSIONS } from "./web_authn/Webauthn";

export interface AuthNoneProps {
  onAuthenticated: () => void;
  onError?: (error: any) => void;
}

export function AuthNone(props: AuthNoneProps) {
  const { onAuthenticated, onError } = props;
  const [error, setError] = useState<string | null>(null);
  const onAuthenticatedRef = useRef(onAuthenticated);
  const onErrorRef = useRef(onError);
  onAuthenticatedRef.current = onAuthenticated;
  onErrorRef.current = onError;

  useEffect(() => {
    let cancelled = false;

    void runAuthNone({
      getClient: getClientInstance,
      permissions: DEFAULT_PERMISSIONS,
      isCancelled: () => cancelled,
      onAuthenticated: () => {
        onAuthenticatedRef.current();
      },
      onError: (authError) => {
        setError(authError.message);
        onErrorRef.current?.(authError);
      },
    });

    return () => {
      cancelled = true;
    };
  }, []);

  if (error) {
    return <Typography color="error">Authentication failed: {error}</Typography>;
  }

  return <Loading />;
}
