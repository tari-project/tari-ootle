//   Copyright 2026 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

import { authenticateClient, isValidJwt } from "@utils/json_rpc";
import useAuthStore from "@store/authStore";
import { useEffect, useState } from "react";
import Loading from "@components/Loading";
import { useNavigate, useSearchParams } from "react-router-dom";

export function AuthNone() {
  const [error, setError] = useState<Error | null>(null);
  const { authToken, setAuthToken } = useAuthStore();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const redirectQuery = searchParams.get("redirect");
  const redirect = redirectQuery ? redirectQuery : "/";

  useEffect(() => {
    if (!isValidJwt(authToken)) {
      authenticateClient("None")
        .then((client) => {
          const token = client.getToken();
          if (token) {
            setAuthToken(token);
            navigate(redirect);
          } else {
            setError(new Error("Failed to authenticate client"));
          }
        })
        .catch((error) => {
          setError(error);
        });
    }
  }, [authToken]);

  if (error) {
    return <div>Error: {error.message}</div>;
  }

  return <Loading />;
}
