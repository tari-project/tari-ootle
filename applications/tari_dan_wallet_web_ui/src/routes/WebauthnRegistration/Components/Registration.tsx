// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import Box from "@mui/material/Box";
import Typography from "@mui/material/Typography";
import {Form} from "react-router-dom";
import TextField from "@mui/material/TextField/TextField";
import Button from "@mui/material/Button";
import Grid from "@mui/material/Grid";
import {useTheme} from "@mui/material/styles";
import {FormEvent, useState} from "react";
import {webauthnStartRegistration} from "../../../utils/json_rpc";
import {Buffer} from "buffer";
import Loading from "../../../Components/Loading";
import useAuthStore from "../../../store/authStore";

const createCredential = async (rpOptions: {rpId: string, rpName: string}, username: string, challenge: Buffer) => {
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
        challenge: challenge,
        pubKeyCredParams: [
            {
                type: 'public-key',
                alg: -7,
            },
            {
                type: 'public-key',
                alg: -257,
            },
        ],
        timeout: 60000,
        excludeCredentials: [],
        attestation: 'none',
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
    const [registrationFormState, setRegistrationFormState] = useState({
        username: "",
    });
    const [loading, setLoading] = useState(false);
    const [error, setError] = useState('');
    const {authToken, setAuthToken} = useAuthStore();

    const onUsernameChange = (e: React.ChangeEvent<HTMLInputElement>) => {
        setRegistrationFormState({
            ...registrationFormState,
            [e.target.name]: e.target.value,
        });
    };

    const handleRegister = async (e: FormEvent) => {
        e.preventDefault();

        setLoading(true);

        const username = registrationFormState.username;

        // start registration by getting challenge
        const startRegisterResponse = await webauthnStartRegistration({
            username: registrationFormState.username,
        }).catch(reason => {
            setLoading(false);
            setError(reason);
            console.error(reason);
        });

        if (!startRegisterResponse) {
            return;
        }

        const challenge = Buffer.from(JSON.parse(startRegisterResponse.public_key).challenge, 'base64');
        const regSessionId = startRegisterResponse.session_id;

        console.log("Session ID:", regSessionId, "Challenge:", challenge);

        // get credential
        const credential = await createCredential(
            {
                rpId: 'localhost',
                rpName: 'Tari Ootle'
            },
            username,
            challenge,
        ).catch(reason => {
            setLoading(false);
            setError(reason);
            console.error(reason);
        });

        if (!credential) {
            return;
        }

        console.log(credential);

        setAuthToken("something");

        // const finishRegisterResponse = await webauthnFinishRegistration({credential: JSON.stringify(credential), session_id: regSessionId})
        //     .catch(reason => {
        //         setLoading(false);
        //         setError(reason);
        //         console.error(reason);
        // });
        //
        // if (!finishRegisterResponse) {
        //     return;
        // }
        //
        // console.log(finishRegisterResponse);
    };

    const errorMessage = (error) ? (
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

    const loadingBar = loading ? (
        <Loading />
    ) : null;

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
                            Register your Webauthn account to get started
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
                            <TextField
                                name="username"
                                disabled={loading}
                                label="Wallet Daemon Username"
                                value={registrationFormState.username}
                                onChange={onUsernameChange}
                                style={{ flexGrow: 1 }}
                                required
                            />
                            <Button variant="contained" type="submit" disabled={loading}>
                                Register
                            </Button>
                            {loadingBar}
                        </Form>
                    </Box>
                </Box>
            </Grid>
        </>
    )
}

export default WebauthnRegistration;