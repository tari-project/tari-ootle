import type { FunctionDef } from "../FunctionDef";
export interface AuthoredTemplate {
    key_index: bigint;
    address: string;
    name: string;
    tari_version: string;
    functions: Array<FunctionDef>;
}
