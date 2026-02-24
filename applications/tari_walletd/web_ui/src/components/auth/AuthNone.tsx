//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { getClientInstance } from "@utils/json_rpc";
import { useEffect, useState } from "react";
import Loading from "@components/Loading";
import { DEFAULT_PERMISSIONS } from "./web_authn/Webauthn";
import { Typography } from "@mui/material";

export interface AuthNoneProps {
  onAuthenticated: () => void;
  onError?: (error: any) => void;
}

export function AuthNone(props: AuthNoneProps) {
  const { onAuthenticated, onError } = props;
  const [error, setError] = useState<string | null>(null);

  useEffect(() => {
    async function authenticate() {
      const client = await getClientInstance();
      const token = await client.authRequest(DEFAULT_PERMISSIONS, "None");
      client.setToken(token);
    }

    if (!error) {
      authenticate()
        .then(() => {
          onAuthenticated();
        })
        .catch((err) => {
          const error = err instanceof Error ? err : new Error(String(err ?? "Unknown authentication error"));
          setError(error.message);
          if (onError) {
            onError(error);
          }
        });
    }
  }, [error]);

  if (error) {
    return <Typography color="error">Authentication failed: {error}</Typography>;
  }

  return <Loading />;
}
