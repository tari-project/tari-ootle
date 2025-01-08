export interface TransactionSealSignature {
    public_key: string;
    signature: {
        public_nonce: string;
        signature: string;
    };
}
