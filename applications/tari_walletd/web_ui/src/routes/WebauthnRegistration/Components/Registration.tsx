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
import Loading from "@components/Loading";
import useAuthStore from "../../../store/authStore";

const WEBAUTHN_RP_ID = import.meta.env.VITE_DAEMON_WEBAUTHN_RP_ID || window.location.hostname;

const createCredential = async (
  rpOptions: { rpId: string; rpName: string },
  username: string,
  challenge: BufferSource,
) => {
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
    authenticatorSelection: {
      userVerification: "required",
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

    try {
      // start registration by getting challenge
      const startRegisterResponse = await webauthnStartRegistration({
        username,
      });

      if (!startRegisterResponse) {
        throw new Error("Failed to start registration");
      }

      if (!startRegisterResponse.public_key) {
        throw new Error("Failed to start registration: missing public_key");
      }

      const challenge = Buffer.from((startRegisterResponse.public_key as any).challenge, "base64");
      const regSessionId = startRegisterResponse.session_id;

      // get credential
      const credential = await createCredential(
        {
          rpId: WEBAUTHN_RP_ID,
          rpName: "Tari Ootle Wallet",
        },
        username,
        challenge,
      );

      if (!credential) {
        throw new Error("Failed to create credential");
      }

      await webauthnFinishRegistration({
        credential,
        session_id: regSessionId,
      });

      navigate("/auth");
    } catch (error) {
      console.error("Error registering WebAuthn key:", error);
      if (error instanceof Error) {
        setError(error.message);
      } else {
        setError(error as string);
      }
    } finally {
      setLoading(false);
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
