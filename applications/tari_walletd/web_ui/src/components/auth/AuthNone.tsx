//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import Loading from "@components/Loading";
import { Typography } from "@mui/material";
import { getClientInstance } from "@utils/json_rpc";
import { useEffect, useState } from "react";
import { DEFAULT_PERMISSIONS } from "./web_authn/Webauthn";

export interface AuthNoneProps {
  onAuthenticated: () => void;
  onError?: (error: any) => void;
}

export function AuthNone(props: AuthNoneProps) {
  const { onAuthenticated, onError } = props;
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    async function authenticate() {
      try {
        const client = await getClientInstance();
        const token = await client.authRequest(DEFAULT_PERMISSIONS, "None");
        client.setToken(token);
        onAuthenticated();
      } catch (err) {
        const error = err instanceof Error ? err : new Error(String(err ?? "Unknown authentication error"));
        setError(error.message);
        if (onError) {
          onError(error);
        }
      }
    }

    authenticate();
  }, [onAuthenticated, onError]);

  if (error) {
    return <Typography color="error">Authentication failed: {error}</Typography>;
  }

  return <Loading />;
}
