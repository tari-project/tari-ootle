import type { Amount } from "./Amount";
import type { ValidatorFeePoolAddress } from "./ValidatorFeePoolAddress";
export interface ValidatorFeeWithdrawal {
    address: ValidatorFeePoolAddress;
    amount: Amount;
}
