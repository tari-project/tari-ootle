import type { Amount } from "./Amount";
export interface ValidatorFeePool {
    claim_public_key: ArrayBuffer;
    amount: Amount;
}
