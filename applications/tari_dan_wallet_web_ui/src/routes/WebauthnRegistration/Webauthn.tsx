// Copyright 2025 The Tari Project
// SPDX-License-Identifier: BSD-3-Clause

import {useEffect, useState} from "react";
import WebauthnLogin from "./Components/Login";
import {useWebauthnAlreadyRegistered} from "../../api/hooks/useWebauthn";
import Loading from "../../Components/Loading";
import WebauthnRegistration from "./Components/Registration";

function Webauthn() {
    const [registered, setRegistered] = useState(false);
    const { data: alreadyRegisteredResponse, isLoading: alreadyRegisteredIsLoading, isError: alreadyRegisteredIsError, error: alreadyRegisteredError } = useWebauthnAlreadyRegistered();

    useEffect(() => {
        if (!alreadyRegisteredIsError && alreadyRegisteredResponse) {
            setRegistered(alreadyRegisteredResponse.registered);
        }

        if (alreadyRegisteredIsError) {
            console.error(alreadyRegisteredError);
        }
    }, [alreadyRegisteredResponse, alreadyRegisteredIsError]);

    if (alreadyRegisteredIsLoading) {
        return <Loading />
    }

    if (!alreadyRegisteredIsLoading && !registered) {
        return <WebauthnRegistration />;
    }

    return <WebauthnLogin />;
}

export default Webauthn;