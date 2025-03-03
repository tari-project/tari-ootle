import type { Substate } from "./Substate";
import type { SubstateId } from "./SubstateId";
import type { ValidatorFeeWithdrawal } from "./ValidatorFeeWithdrawal";
export interface SubstateDiff {
    up_substates: Array<[SubstateId, Substate]>;
    down_substates: Array<[SubstateId, number]>;
    fee_withdrawals: Array<ValidatorFeeWithdrawal>;
}
