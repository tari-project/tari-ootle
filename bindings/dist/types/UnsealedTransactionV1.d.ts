import type { TransactionSignature } from "./TransactionSignature";
import type { UnsignedTransactionV1 } from "./UnsignedTransactionV1";
export interface UnsealedTransactionV1 {
    transaction: UnsignedTransactionV1;
    signatures: Array<TransactionSignature>;
}
