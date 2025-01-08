import type { Epoch } from "../Epoch";
import type { ValidatorNodeChange } from "./ValidatorNodeChange";
export interface GetBaseLayerEpochChangesResponse {
    changes: Array<[Epoch, Array<ValidatorNodeChange>]>;
}
