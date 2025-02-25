import type { Amount } from "../Amount";
import type { ComponentAddressOrName } from "./ComponentAddressOrName";
import type { Shard } from "../Shard";
export interface ClaimValidatorFeesRequest {
    account: ComponentAddressOrName | null;
    claim_key_index: number | null;
    max_fee: Amount | null;
    shards: Array<Shard>;
    dry_run: boolean;
}
