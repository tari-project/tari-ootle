import type { FeePoolDetails } from "./FeePoolDetails";
import type { Shard } from "../Shard";
export interface GetValidatorFeesResponse {
    fees: Record<Shard, FeePoolDetails>;
}
