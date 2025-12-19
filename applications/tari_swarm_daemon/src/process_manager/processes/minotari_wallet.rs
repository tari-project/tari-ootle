//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::{fs::File, path::PathBuf};

use anyhow::anyhow;
use minotari_node_grpc_client::grpc;
use minotari_wallet_grpc_client::WalletGrpcClient;
use serde::Serialize;
use tari_crypto::tari_utilities::ByteArray;
use tari_engine_types::confidential::{AbridgedTransactionKernel, EncodedMerkleProof, MinotariBurnClaimProof};
use tari_template_lib_types::{
    crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes, Scalar32Bytes, SchnorrSignatureBytes},
    EncryptedData,
};
use tari_transaction_components::transaction_components::{memo_field::TxType, MemoField};
use tari_wallet_daemon_client::types::ClaimBurnProof;

use crate::process_manager::Instance;

pub struct MinoTariWalletProcess {
    instance: Instance,
}

impl MinoTariWalletProcess {
    pub fn new(instance: Instance) -> Self {
        Self { instance }
    }

    pub fn instance(&self) -> &Instance {
        &self.instance
    }

    pub fn instance_mut(&mut self) -> &mut Instance {
        &mut self.instance
    }

    pub async fn connect_client(&self) -> anyhow::Result<WalletGrpcClient<tonic::transport::Channel>> {
        let port = self
            .instance
            .allocated_ports()
            .get("grpc")
            .ok_or_else(|| anyhow!("No wallet port allocated"))?;
        let client = WalletGrpcClient::connect(&format!("http://localhost:{}", port)).await?;
        Ok(client)
    }

    pub async fn burn_funds(
        &self,
        amount: u64,
        claim_public_key: RistrettoPublicKeyBytes,
    ) -> anyhow::Result<BurnClaimProofResponse> {
        let mut client = self.connect_client().await?;

        let request = grpc::CreateBurnTransactionRequest {
            amount,
            fee_per_gram: 1,
            payment_id: MemoField::new_open("Burn funds in swarm".as_bytes().to_vec(), TxType::Burn)
                .map_err(|e| anyhow!("Failed to create MemoField: {e}"))?
                .to_bytes(),
            claim_public_key: claim_public_key.to_vec(),
            sidechain_deployment_key: vec![],
        };
        let resp = client.create_burn_transaction(request).await?;
        let resp = resp.into_inner();
        if !resp.is_success {
            return Err(anyhow!("Failed to burn funds: {}", resp.failure_message));
        }

        let commitment = PedersenCommitmentBytes::from_bytes(&resp.commitment)
            .map_err(|e| anyhow!("commitment parse error: {e}"))?;

        let resp = BurnClaimProofResponse {
            tx_id: resp.transaction_id,
            claim_public_key,
            commitment,
        };

        Ok(resp)
    }

    pub async fn wait_for_claim_burn_proof_task(
        mut client: WalletGrpcClient<tonic::transport::Channel>,
        path: PathBuf,
        commitment: PedersenCommitmentBytes,
        value: u64,
        nonce_key_index: u64,
    ) -> anyhow::Result<()> {
        let proof = loop {
            let resp = client
                .get_burn_claim_proof(grpc::GetBurnClaimProofRequest {
                    commitment: commitment.as_bytes().to_vec(),
                })
                .await?;
            let resp = resp.into_inner();
            if let Some(merkle_proof) = resp.merkle_proof {
                let claim_proof = resp.claim_proof.ok_or_else(|| anyhow!("No claim proof in response"))?;
                let ownership_proof = claim_proof
                    .ownership_proof
                    .ok_or_else(|| anyhow!("No ownership proof in response"))?;
                let commitment = PedersenCommitmentBytes::from_bytes(&claim_proof.commitment)
                    .map_err(|e| anyhow!("commitment parse error: {e}"))?;

                let ownership_proof = SchnorrSignatureBytes::new(
                    RistrettoPublicKeyBytes::from_bytes(&ownership_proof.public_nonce)
                        .map_err(|e| anyhow!("sig public_nonce parse error {e}"))?,
                    Scalar32Bytes::from_bytes(&ownership_proof.signature)
                        .map_err(|e| anyhow!("sig parse error {e}"))?,
                );

                let reciprocal_claim_public_key =
                    RistrettoPublicKeyBytes::from_bytes(&claim_proof.reciprocal_claim_public_key)
                        .map_err(|e| anyhow!("reciprocal_claim_public_key parse error {e}"))?;
                let kernel = resp.kernel.ok_or_else(|| anyhow!("No kernel in response"))?;
                let kernel = AbridgedTransactionKernel {
                    version: kernel.version as u8,
                    fee: kernel.fee,
                    lock_height: kernel.lock_height,
                    excess: kernel
                        .excess
                        .as_slice()
                        .try_into()
                        .map_err(|e| anyhow!("excess parse error: {e}"))?,
                    excess_sig: {
                        let excess_sig = kernel
                            .excess_sig
                            .as_ref()
                            .ok_or_else(|| anyhow!("No excess_sig in response"))?;

                        SchnorrSignatureBytes::new(
                            excess_sig
                                .public_nonce
                                .as_slice()
                                .try_into()
                                .map_err(|e| anyhow!("excess_sig parse error: {e}"))?,
                            excess_sig
                                .signature
                                .as_slice()
                                .try_into()
                                .map_err(|e| anyhow!("excess_sig parse error: {e}"))?,
                        )
                    },
                };

                break ClaimBurnProof {
                    claim_proof: MinotariBurnClaimProof {
                        burn_public_key: reciprocal_claim_public_key,
                        commitment,
                        ownership_proof,
                        encoded_merkle_proof: EncodedMerkleProof {
                            block_hash: merkle_proof.block_hash.as_slice().try_into().map_err(|e| {
                                anyhow!(
                                    "Block hash length {} is out of bounds: {e}",
                                    merkle_proof.block_hash.len()
                                )
                            })?,
                            encoded_merkle_proof: merkle_proof
                                .encoded_proof
                                .try_into()
                                .map_err(|e| anyhow!("Encoded merkle proof length is out of bounds: {e}"))?,
                            leaf_index: merkle_proof.leaf_index,
                        },
                        kernel,
                        value,
                    },
                    owner_nonce_key_index: nonce_key_index,
                    encrypted_data: EncryptedData::try_from(resp.encrypted_data)
                        .map_err(|e| anyhow!("Encrypted data length is out of bounds: {e}",))?,
                };
            };

            tokio::time::sleep(std::time::Duration::from_secs(1)).await;
        };

        let mut file = File::create(&path)?;
        serde_json::to_writer_pretty(&mut file, &proof)?;

        Ok(())
    }

    pub async fn get_balance(&self) -> anyhow::Result<grpc::GetBalanceResponse> {
        let mut client = self.connect_client().await?;
        let balance = client
            .get_balance(grpc::GetBalanceRequest { payment_id: None })
            .await?
            .into_inner();
        Ok(balance)
    }

    pub async fn revalidate_all_transactions(&self) -> anyhow::Result<()> {
        let mut client = self.connect_client().await?;
        client.revalidate_all_transactions(grpc::RevalidateRequest {}).await?;
        Ok(())
    }

    pub async fn get_transaction_info(&self, transaction_ids: Vec<u64>) -> anyhow::Result<Vec<grpc::TransactionInfo>> {
        let mut client = self.connect_client().await?;
        let identity = client
            .get_transaction_info(grpc::GetTransactionInfoRequest { transaction_ids })
            .await?
            .into_inner();
        Ok(identity.transactions)
    }
}

#[derive(Debug, Clone, Serialize)]
pub struct BurnClaimProofResponse {
    pub tx_id: u64,
    pub claim_public_key: RistrettoPublicKeyBytes,
    pub commitment: PedersenCommitmentBytes,
}
