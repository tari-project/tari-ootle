import type { BlockHeader } from "./BlockHeader";
import type { Command } from "./Command";
import type { QuorumCertificate } from "./QuorumCertificate";
export interface Block {
    header: BlockHeader;
    justify: QuorumCertificate;
    commands: Array<Command>;
    is_justified: boolean;
    is_committed: boolean;
    block_time: number | null;
    stored_at: Array<number> | null;
}
