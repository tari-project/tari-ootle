export interface ShardGroupEvidence {
    inputs: Record<string, any>;
    outputs: Record<string, number>;
    prepare_qc: string | null;
    accept_qc: string | null;
}
