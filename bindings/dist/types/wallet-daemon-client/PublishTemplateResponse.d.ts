import type { Amount } from "../Amount";
export interface PublishTemplateResponse {
    transaction_id: string;
    dry_run_fee: Amount | null;
}
