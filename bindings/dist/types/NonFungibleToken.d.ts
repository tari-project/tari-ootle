import type { NonFungibleId } from "./NonFungibleId";
import type { ResourceAddress } from "./ResourceAddress";
import type { VaultId } from "./VaultId";
export interface NonFungibleToken {
    vault_id: VaultId;
    nft_id: NonFungibleId;
    resource_address: ResourceAddress;
    data: any;
    mutable_data: any;
    is_burned: boolean;
}
