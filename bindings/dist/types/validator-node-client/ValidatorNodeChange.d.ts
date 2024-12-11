import type { Epoch } from "../Epoch";
export type ValidatorNodeChange = {
    Add: {
        public_key: string;
        activation_epoch: Epoch;
        minimum_value_promise: bigint;
    };
} | {
    Remove: {
        public_key: string;
    };
};
