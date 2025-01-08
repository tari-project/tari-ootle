import type { ComponentAddressOrName } from "./ComponentAddressOrName";
export interface PublishTransactionRequest {
    binary: Array<number>;
    fee_account: ComponentAddressOrName;
    max_fee: number;
    detect_inputs: boolean;
}
