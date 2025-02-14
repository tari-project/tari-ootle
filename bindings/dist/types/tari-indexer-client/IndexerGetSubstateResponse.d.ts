import type { SubstateId } from "../SubstateId";
import type { SubstateValue } from "../SubstateValue";
export interface IndexerGetSubstateResponse {
    address: SubstateId;
    version: number;
    substate: SubstateValue;
    created_by_transaction: string;
}
