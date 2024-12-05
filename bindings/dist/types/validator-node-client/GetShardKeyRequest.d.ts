import type { Epoch } from "../Epoch";
export interface GetShardKeyRequest {
    epoch: Epoch;
    public_key: string;
}
