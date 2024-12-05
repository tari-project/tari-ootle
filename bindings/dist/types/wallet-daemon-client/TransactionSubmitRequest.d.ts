import type { SubstateRequirement } from "../SubstateRequirement";
import type { UnsignedTransaction } from "../UnsignedTransaction";
export interface TransactionSubmitRequest {
    transaction: UnsignedTransaction;
    signing_key_index: number | null;
    autofill_inputs: Array<SubstateRequirement>;
    detect_inputs: boolean;
    detect_inputs_use_unversioned: boolean;
    proof_ids: Array<number>;
}
