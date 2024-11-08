import type { Epoch } from "./Epoch";
import type { ExtraData } from "./ExtraData";
import type { NodeHeight } from "./NodeHeight";
import type { Shard } from "./Shard";
import type { ShardGroup } from "./ShardGroup";
export interface BlockHeader {
    id: string;
    network: string;
    parent: string;
    justify_id: string;
    height: NodeHeight;
    epoch: Epoch;
    shard_group: ShardGroup;
    proposed_by: string;
    total_leader_fee: number;
    state_merkle_root: string;
    command_merkle_root: string;
    is_dummy: boolean;
    foreign_indexes: Record<Shard, bigint>;
    signature: {
        public_nonce: string;
        signature: string;
    } | null;
    timestamp: number;
    base_layer_block_height: number;
    base_layer_block_hash: string;
    extra_data: ExtraData;
}
