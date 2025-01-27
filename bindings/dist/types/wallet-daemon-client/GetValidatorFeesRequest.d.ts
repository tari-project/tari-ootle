import type { AccountOrKeyIndex } from "./AccountOrKeyIndex";
import type { ShardGroup } from "../ShardGroup";
export interface GetValidatorFeesRequest {
    account_or_key: AccountOrKeyIndex;
    shard_group: ShardGroup | null;
}
