import type { Epoch } from "../Epoch";
import type { NodeHeight } from "../NodeHeight";
export interface GetConsensusStatusResponse {
    epoch: Epoch;
    height: NodeHeight;
    state: string;
}
