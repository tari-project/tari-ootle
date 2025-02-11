import {useQuery} from "@tanstack/react-query";
import {webauthnAlreadyRegistered} from "../../utils/json_rpc";
import {ApiError} from "../helpers/types";

export const useWebauthnAlreadyRegistered = () => {
    return useQuery({
        queryKey: ["webauthn_already_registered"],
        queryFn: () => {
            return webauthnAlreadyRegistered();
        },
        onError: (error: ApiError) => {
            error;
        },
        refetchInterval: false,
        notifyOnChangeProps: ["data", "error"],
        retryOnMount: false,
        retry: false,
    });
};