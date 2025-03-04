import type { WebauthnFinishAuthRequest } from "./WebauthnFinishAuthRequest";
export interface AuthLoginRequest {
    permissions: Array<string>;
    duration: {
        secs: number;
        nanos: number;
    } | null;
    webauthn_finish_auth_request: WebauthnFinishAuthRequest | null;
}
