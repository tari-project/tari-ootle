import type { Epoch } from "../Epoch";
import type { SubstateAddress } from "../SubstateAddress";
export type ValidatorNodeChange = {
    Add: {
        public_key: string;
        activation_epoch: Epoch;
        minimum_value_promise: bigint;
        shard_key: SubstateAddress;
    };
} | {
    Remove: {
        public_key: string;
    };
};
