// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import { useTheme } from "@mui/material/styles";
import { FormEvent, useState } from "react";
import { Form, useNavigate, useSearchParams } from "react-router-dom";
import Loading from "../../../Components/Loading";
import Typography from "@mui/material/Typography";
import Grid from "@mui/material/Grid";
import Box from "@mui/material/Box";
import Button from "@mui/material/Button";
import { unauthenticated_client, webauthnStartAuth } from "../../../utils/json_rpc";
import { Buffer } from "buffer";
import { WebauthnFinishAuthRequest } from "@tari-project/typescript-bindings";
import useAuthStore from "../../../store/authStore";

const getCredential = async (challenge: any, allowCredentials: any) => {
  const publicKeyCredentialRequestOptions: PublicKeyCredentialRequestOptions = {
    challenge: challenge,
    allowCredentials: allowCredentials,
    timeout: 60000,
    userVerification: "required",
  };
  return await navigator.credentials.get({
    publicKey: publicKeyCredentialRequestOptions,
  });
};

function WebauthnLogin() {
  const theme = useTheme();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const navigate = useNavigate();
  const { setAuthToken, username } = useAuthStore();
  const [searchParams] = useSearchParams();
  const redirectQuery = searchParams.get("redirect");
  const redirect = redirectQuery ? redirectQuery : "/";

  const handleLogin = async (e: FormEvent) => {
    e.preventDefault();
    setLoading(true);

    // start authentication by getting challenge
    const startAuthResponse = await webauthnStartAuth({ username }).catch((reason) => {
      setLoading(false);
      setError(reason);
      console.error(reason);
    });

    if (!startAuthResponse) {
      return;
    }

    const challengeResponse = JSON.parse(startAuthResponse.challenge);
    const challenge = Buffer.from(challengeResponse.publicKey.challenge, "base64");
    const loginSessionId = startAuthResponse.session_id;
    let allowCredentials = challengeResponse.publicKey.allowCredentials.map((value: any) => {
      return {
        id: Buffer.from(value.id, "base64"),
        type: value.type,
      };
    });
    let credential = await getCredential(challenge, allowCredentials);

    if (!credential) {
      return;
    }

    const client = await unauthenticated_client();
    const webauthnFinishAuthRequest: WebauthnFinishAuthRequest = {
      credential: JSON.stringify(credential),
      session_id: loginSessionId,
    };
    const authToken = await client.authRequest(["Admin"], webauthnFinishAuthRequest).catch((reason) => {
      setLoading(false);
      setError(reason);
      console.error(reason);
    });

    if (!authToken) {
      return;
    }

    const acceptToken = await client.authAccept(authToken, authToken);

    setAuthToken(acceptToken);
    navigate(redirect);
  };

  const errorMessage = error ? (
    <Typography
      variant="h4"
      style={{
        textAlign: "center",
        color: "red",
      }}
    >
      {error.toString()}
    </Typography>
  ) : null;

  const loadingBar = loading ? <Loading /> : null;

  return (
    <>
      <Grid item xs={12} md={12} lg={12}>
        <Box
          style={{
            display: "flex",
            justifyContent: "center",
            alignItems: "center",
            flexDirection: "column",
            width: "100%",
            height: "calc(100vh - 200px)",
            minHeight: 400,
            gap: theme.spacing(3),
          }}
        >
          <Box
            style={{
              display: "flex",
              justifyContent: "center",
              alignItems: "center",
              flexDirection: "column",
              gap: 0,
              maxWidth: 600,
            }}
          >
            <Typography
              variant="h3"
              style={{
                textAlign: "center",
              }}
            >
              Welcome to the Tari Asset Vault
            </Typography>
            <Typography
              variant="h5"
              style={{
                textAlign: "center",
              }}
            >
              Please login to get access to your wallet
            </Typography>
            {errorMessage}
            <Form
              onSubmit={handleLogin}
              className="flex-container"
              style={{
                flexDirection: "column",
                marginTop: theme.spacing(3),
              }}
            >
              <Button variant="contained" type="submit" disabled={loading}>
                Login
              </Button>
              {loadingBar}
            </Form>
          </Box>
        </Box>
      </Grid>
    </>
  );
}

export default WebauthnLogin;
