import type { ExecuteResult } from "../ExecuteResult";
export interface TransactionSubmitDryRunResponse {
    transaction_id: string;
    result: ExecuteResult;
}
