// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useEffect, useState } from "react";
import WebauthnLogin from "./Components/Login";
import { useWebauthnAlreadyRegistered } from "../../api/hooks/useWebauthn";
import Loading from "@components/Loading";
import WebauthnRegistration from "./Components/Registration";
import useAuthStore from "../../store/authStore";
import { useNavigate, useSearchParams } from "react-router-dom";

function Webauthn() {
  const [registered, setRegistered] = useState(false);
  const { authToken, username } = useAuthStore();
  const {
    data: alreadyRegisteredResponse,
    isLoading: alreadyRegisteredIsLoading,
    isError: alreadyRegisteredIsError,
    error: alreadyRegisteredError,
  } = useWebauthnAlreadyRegistered(username);
  const navigate = useNavigate();
  const [searchParams] = useSearchParams();
  const redirectQuery = searchParams.get("redirect");
  const redirect = redirectQuery ? redirectQuery : "/";

  if (authToken) {
    navigate(redirect);
  }

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
    return <WebauthnRegistration />;
  }

  return <WebauthnLogin />;
}

export default Webauthn;
