import type { Amount } from "../Amount";
import type { ValidatorFeePoolAddress } from "../ValidatorFeePoolAddress";
export interface FeePoolDetails {
    address: ValidatorFeePoolAddress;
    amount: Amount;
}
