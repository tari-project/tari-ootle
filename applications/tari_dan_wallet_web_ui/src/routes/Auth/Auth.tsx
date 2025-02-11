// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useAuthMethod} from "../../api/hooks/useAuth";
import {useEffect, useState} from "react";
import Loading from "../../Components/Loading";
import {Navigate} from "react-router-dom";

function Auth() {
    const { data: authMethod, isError: authMethodsIsError, error: authMethodsError } = useAuthMethod();
    const [ currAuthMethod, setCurrAuthMethod ] = useState('');

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
                    return <Navigate replace to="/" />;
                }
                case 'webauthn': {
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