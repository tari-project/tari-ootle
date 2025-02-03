import type { SubstateValue } from "../SubstateValue";
import type { WalletSubstateRecord } from "./WalletSubstateRecord";
export interface SubstatesGetResponse {
    record: WalletSubstateRecord;
    value: SubstateValue;
}
