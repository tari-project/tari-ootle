// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useEffect, useState } from "react";
import WebauthnLogin from "./Components/Login";
import WebauthnRegistration from "./Components/Registration";
import { useWebauthnAlreadyRegistered } from "@api/hooks/useWebauthn";
import Loading from "@components/Loading";
import useAuthStore from "@store/authStore";
import { useSearchParams } from "react-router-dom";

function Webauthn() {
  console.log("Rendering Webauthn component");
  const [registered, setRegistered] = useState(false);
  const { username } = useAuthStore();
  const {
    data: alreadyRegisteredResponse,
    isLoading: alreadyRegisteredIsLoading,
    isError: alreadyRegisteredIsError,
    error: alreadyRegisteredError,
  } = useWebauthnAlreadyRegistered(username);
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
