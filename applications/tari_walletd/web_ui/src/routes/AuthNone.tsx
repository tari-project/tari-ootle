//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { getClientInstance } from "@utils/json_rpc";
import useAuthStore from "@store/authStore";
import { useEffect, useState } from "react";
import Loading from "@components/Loading";
import { useNavigate, useSearchParams } from "react-router-dom";
import { DEFAULT_PERMISSIONS } from "@routes/Webauthn/Webauthn";

export function AuthNone() {
  const [error, setError] = useState<Error | null>(null);
  const { loggedIn, setLoggedIn } = useAuthStore();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const redirectQuery = searchParams.get("redirect");
  const redirect = redirectQuery ? redirectQuery : "/";

  useEffect(() => {
    async function authenticate() {
      const client = await getClientInstance();
      const token = await client.authRequest(DEFAULT_PERMISSIONS, "None");
      client.setToken(token);
    }

    if (!loggedIn) {
      authenticate()
        .then(() => {
          setLoggedIn(true);
          navigate(redirect);
        })
        .catch((error) => {
          setError(error);
        });
    }
  }, [loggedIn]);

  if (error) {
    return <div>Error: {error.message}</div>;
  }

  return <Loading />;
}
