import type { TransactionSealSignature } from "./TransactionSealSignature";
import type { UnsealedTransactionV1 } from "./UnsealedTransactionV1";
import type { VersionedSubstateId } from "./VersionedSubstateId";
export interface TransactionV1 {
    id: string;
    body: UnsealedTransactionV1;
    seal_signature: TransactionSealSignature;
    filled_inputs: Array<VersionedSubstateId>;
}
