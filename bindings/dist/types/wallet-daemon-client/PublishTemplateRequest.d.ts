import type { ComponentAddressOrName } from "./ComponentAddressOrName";
export interface PublishTemplateRequest {
    binary: string;
    fee_account: ComponentAddressOrName | null;
    max_fee: number;
    detect_inputs: boolean;
    dry_run: boolean;
}
