// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import { Form, useNavigate } from "react-router-dom";
import Button from "@mui/material/Button";
import Grid from "@mui/material/Grid";
import { useTheme } from "@mui/material/styles";
import { FormEvent, useState } from "react";
import { webauthnFinishRegistration, webauthnStartRegistration } from "../../../utils/json_rpc";
import { Buffer } from "buffer";
import Loading from "../../../Components/Loading";
import useAuthStore from "../../../store/authStore";

const WEBAUTHN_RP_ID = import.meta.env.VITE_DAEMON_WEBAUTHN_RP_ID || window.location.hostname;

const createCredential = async (rpOptions: { rpId: string; rpName: string }, username: string, challenge: Buffer) => {
  const publicKeyCredentialCreationOptions: PublicKeyCredentialCreationOptions = {
    rp: {
      name: rpOptions.rpName,
      id: rpOptions.rpId,
    },
    user: {
      id: Uint8Array.from(username, (c) => c.charCodeAt(0)),
      name: username,
      displayName: username,
    },
    challenge,
    pubKeyCredParams: [
      {
        type: "public-key",
        // Ed25519
        alg: -8,
      },
      {
        type: "public-key",
        // ES256
        alg: -7,
      },
      {
        type: "public-key",
        // RS256
        alg: -257,
      },
    ],
    timeout: 60000,
    excludeCredentials: [],
    attestation: "none",
    extensions: {
      credProps: true,
    },
  };
  return await navigator.credentials.create({
    publicKey: publicKeyCredentialCreationOptions,
  });
};

function WebauthnRegistration() {
  const theme = useTheme();
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState("");
  const { username } = useAuthStore();
  const navigate = useNavigate();

  const handleRegister = async (e: FormEvent) => {
    e.preventDefault();

    setLoading(true);

    // start registration by getting challenge
    const startRegisterResponse = await webauthnStartRegistration({
      username,
    })
      .catch((reason) => {
        setError(reason);
        console.error(reason);
      })
      .finally(() => {
        setLoading(false);
      });

    if (!startRegisterResponse) {
      return;
    }

    const challenge = Buffer.from(JSON.parse(startRegisterResponse.public_key).challenge, "base64");
    const regSessionId = startRegisterResponse.session_id;

    // get credential
    const credential = await createCredential(
      {
        rpId: WEBAUTHN_RP_ID,
        rpName: "Tari Ootle Wallet",
      },
      username,
      challenge,
    ).catch((reason) => {
      setLoading(false);
      setError(reason);
      console.error(reason);
    });

    if (!credential) {
      return;
    }

    const finishRegisterResponse = await webauthnFinishRegistration({
      credential: JSON.stringify(credential),
      session_id: regSessionId,
    }).catch((reason) => {
      setLoading(false);
      setError(reason);
      console.error(reason);
    });

    if (!finishRegisterResponse) {
      return;
    }

    if (finishRegisterResponse.success) {
      navigate("/auth");
    }
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
              Register your{" "}
              <a href="https://webauthn.io/" target="_blank">
                WebAuthn
              </a>{" "}
              key to get started
            </Typography>
            {errorMessage}
            <Form
              onSubmit={handleRegister}
              className="flex-container"
              style={{
                flexDirection: "column",
                marginTop: theme.spacing(3),
              }}
            >
              <Button variant="contained" type="submit" disabled={loading}>
                Register
              </Button>
              {loadingBar}
            </Form>
          </Box>
        </Box>
      </Grid>
    </>
  );
}

export default WebauthnRegistration;
