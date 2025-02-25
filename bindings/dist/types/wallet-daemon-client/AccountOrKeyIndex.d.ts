import type { ComponentAddressOrName } from "./ComponentAddressOrName";
export type AccountOrKeyIndex = {
    Account: ComponentAddressOrName | null;
} | {
    KeyIndex: number;
};
