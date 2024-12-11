import type { Epoch } from "../Epoch";
export interface GetBaseLayerEpochChangesRequest {
    start_epoch: Epoch;
    end_epoch: Epoch;
}
