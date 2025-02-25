import type { ComponentHeader } from "./ComponentHeader";
import type { NonFungibleContainer } from "./NonFungibleContainer";
import type { NonFungibleIndex } from "./NonFungibleIndex";
import type { PublishedTemplate } from "./PublishedTemplate";
import type { Resource } from "./Resource";
import type { TransactionReceipt } from "./TransactionReceipt";
import type { UnclaimedConfidentialOutput } from "./UnclaimedConfidentialOutput";
import type { ValidatorFeePool } from "./ValidatorFeePool";
import type { Vault } from "./Vault";
export type SubstateValue = {
    Component: ComponentHeader;
} | {
    Resource: Resource;
} | {
    Vault: Vault;
} | {
    NonFungible: NonFungibleContainer;
} | {
    NonFungibleIndex: NonFungibleIndex;
} | {
    UnclaimedConfidentialOutput: UnclaimedConfidentialOutput;
} | {
    TransactionReceipt: TransactionReceipt;
} | {
    Template: PublishedTemplate;
} | {
    ValidatorFeePool: ValidatorFeePool;
};
