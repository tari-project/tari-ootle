// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useAuthMethod} from "../../api/hooks/useAuth";
import {useEffect, useState} from "react";
import Loading from "../../Components/Loading";
import {Navigate, useNavigate} from "react-router-dom";
import useAuthStore from "../../store/authStore";

export const AUTH_TOKEN_FOR_NONE_AUTH: string = "auth_none";

function Auth() {
    const { data: authMethod, isError: authMethodsIsError, error: authMethodsError } = useAuthMethod();
    const [ currAuthMethod, setCurrAuthMethod ] = useState('');
    const {authToken, setAuthToken} = useAuthStore();
    const navigate = useNavigate();

    if (authToken) {
        navigate("/");
    }

    useEffect(() => {
        if (!authMethodsIsError && authMethod) {
            setCurrAuthMethod(authMethod.method);
        }

        if (authMethodsError) {
            console.error(authMethodsError);
        }
    }, [authMethod, authMethodsIsError]);

    const auth = (() => {
            switch(currAuthMethod) {
                case 'none': {
                    setAuthToken(AUTH_TOKEN_FOR_NONE_AUTH);
                    return <Navigate replace to="/" />;
                }
                case 'webauthn': {
                    if (authToken === AUTH_TOKEN_FOR_NONE_AUTH) {
                        setAuthToken("");
                    }
                    return <Navigate replace to="/auth/webauthn" />;
                }
                default: {
                    return <Loading />;
                }
            }
    })();

    return auth;
}

export default Auth;