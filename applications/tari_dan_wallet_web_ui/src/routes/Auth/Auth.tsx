// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useAuthMethod } from "../../api/hooks/useAuth";
import { useEffect, useState } from "react";
import Loading from "../../Components/Loading";
import { Navigate, useNavigate, useSearchParams } from "react-router-dom";
import useAuthStore from "../../store/authStore";

export const AUTH_TOKEN_FOR_NONE_AUTH: string = "auth_none";

function Auth() {
  const { data: authMethod, isError: authMethodsIsError, error: authMethodsError } = useAuthMethod();
  const [currAuthMethod, setCurrAuthMethod] = useState("");
  const { authToken, setAuthToken } = useAuthStore();
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const redirectQuery = searchParams.get("redirect");
  const redirect = redirectQuery ? redirectQuery : "/";

  if (authToken) {
    navigate(redirect);
  }

  useEffect(() => {
    if (!authMethodsIsError && authMethod) {
      setCurrAuthMethod(authMethod.method);
    }

    if (authMethodsError) {
      console.error(authMethodsError);
    }
  }, [authMethod, authMethodsIsError]);

  if (currAuthMethod === "none") {
    console.log("no auth");
    setAuthToken(AUTH_TOKEN_FOR_NONE_AUTH);
    return <Navigate replace to={redirect} />;
  }

  if (currAuthMethod === "webauthn") {
    if (authToken === AUTH_TOKEN_FOR_NONE_AUTH) {
      setAuthToken("");
    }
    return <Navigate replace to={"/auth/webauthn?redirect=" + redirect} />;
  }

  return <Loading />;
}

export default Auth;
