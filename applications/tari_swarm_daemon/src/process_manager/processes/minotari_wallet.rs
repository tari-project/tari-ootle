//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use std::path::{Path, PathBuf};

use anyhow::anyhow;
use log::{debug, error, info};
use minotari_node_grpc_client::grpc::{self, RevalidateRequest};
use minotari_wallet_grpc_client::WalletGrpcClient;
use serde::Serialize;
use tari_common_types::types::CompressedPublicKey;
use tari_crypto::{
    ristretto::{CompressedRistrettoSchnorr, RistrettoSecretKey},
    tari_utilities::ByteArray,
};
use tari_ootle_app_utilities::burn_claim_proof::burn_output_proof_from_l1;
use tari_sidechain::{BurnClaimProof, CompleteClaimBurnProof};
use tari_sidechain_l1::BurnOutputProof as L1BurnOutputProof;
use tari_template_lib_types::crypto::{PedersenCommitmentBytes, RistrettoPublicKeyBytes};
use tari_transaction_components::transaction_components::{MemoField, memo_field::TxType};

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
            error!("Burn funds failed: {}", resp.failure_message);
            return Err(anyhow!("Failed to burn funds: {}", resp.failure_message));
        }
        info!("Burn transaction created with ID: {}", resp.transaction_id);

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
        client: WalletGrpcClient<tonic::transport::Channel>,
        path: PathBuf,
        commitment: PedersenCommitmentBytes,
        value: u64,
    ) {
        if let Err(e) = Self::wait_for_claim_burn_proof_task_inner(client, path, commitment, value).await {
            error!("Error while waiting for burn claim proof: {}", e);
            return;
        }
        info!("Burn claim proof written to file.");
    }

    async fn wait_for_claim_burn_proof_task_inner(
        mut client: WalletGrpcClient<tonic::transport::Channel>,
        path: PathBuf,
        commitment: PedersenCommitmentBytes,
        value: u64,
    ) -> anyhow::Result<()> {
        let mut attempts = 0;
        loop {
            let resp = client
                .get_burn_claim_proof(grpc::GetBurnClaimProofRequest {
                    commitment: commitment.as_bytes().to_vec(),
                })
                .await?;
            let resp = resp.into_inner();
            let (Some(output_proof), Some(mined_in_epoch)) = (resp.burn_output_proof, resp.mined_in_epoch) else {
                attempts += 1;
                tokio::time::sleep(std::time::Duration::from_secs(2)).await;
                info!("Waiting for burn claim proof... attempt {}", attempts);
                continue;
            };

            info!("Received burn claim proof from wallet.");

            let claim_proof = resp.claim_proof.ok_or_else(|| anyhow!("No claim proof in response"))?;
            let ownership_proof = claim_proof
                .ownership_proof
                .ok_or_else(|| anyhow!("No ownership proof in response"))?;
            let ownership_proof = CompressedRistrettoSchnorr::new(
                CompressedPublicKey::from_canonical_bytes(&ownership_proof.public_nonce)
                    .map_err(|e| anyhow!("sig public_nonce parse error {e}"))?,
                RistrettoSecretKey::from_canonical_bytes(&ownership_proof.signature)
                    .map_err(|e| anyhow!("sig parse error {e}"))?,
            );
            let reciprocal_claim_public_key = CompressedPublicKey::from_canonical_bytes(&claim_proof.claim_public_key)
                .map_err(|e| anyhow!("reciprocal_claim_public_key parse error {e}"))?;

            let proof = CompleteClaimBurnProof {
                claim_proof: BurnClaimProof {
                    burn_public_key: reciprocal_claim_public_key,
                    ownership_proof,
                    output_proof: L1BurnOutputProof::try_from(output_proof)
                        .map(burn_output_proof_from_l1)
                        .map_err(|e| anyhow!("burn output proof parse error: {e}"))?,
                    value,
                },
                encrypted_data: resp.encrypted_data,
                mined_in_epoch,
            };

            write_claim_burn_proof_file(&path, &proof).await?;
            break;
        }

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
        client
            .revalidate_all_transactions(RevalidateRequest {
                transaction_mode: 1, // Full mode
                output_mode: 1,      // Full mode
            })
            .await?;
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

async fn write_claim_burn_proof_file(path: &Path, proof: &CompleteClaimBurnProof) -> anyhow::Result<()> {
    let bytes = serde_json::to_vec_pretty(proof)?;
    if let Some(parent) = path.parent().filter(|p| !p.as_os_str().is_empty()) {
        tokio::fs::create_dir_all(parent).await?;
    }
    let tmp_path = path.with_extension("json.tmp");
    tokio::fs::write(&tmp_path, &bytes).await?;
    if let Err(err) = tokio::fs::rename(&tmp_path, path).await {
        if let Err(remove_err) = tokio::fs::remove_file(&tmp_path).await {
            debug!(
                "Failed to remove temporary burn claim proof file {}: {}",
                tmp_path.display(),
                remove_err
            );
        }
        return Err(err.into());
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use tari_sidechain::{BurnOutputProof, MmrInclusionProof, OutputHashPreimage};

    use super::*;

    fn empty_mmr_proof() -> MmrInclusionProof {
        MmrInclusionProof {
            leaf_index: 0,
            mmr_size: 1,
            path: vec![],
            peaks: vec![],
        }
    }

    #[tokio::test]
    async fn write_claim_burn_proof_file_writes_atomically_and_removes_tmp_sibling() {
        let dir = tempfile::tempdir().unwrap();
        let nested = dir.path().join("misc").join("burn_proof-42.json");
        let proof = CompleteClaimBurnProof {
            claim_proof: BurnClaimProof {
                burn_public_key: CompressedPublicKey::default(),
                ownership_proof: CompressedRistrettoSchnorr::default(),
                output_proof: BurnOutputProof {
                    block_hash: Default::default(),
                    block_height: 100,
                    output: OutputHashPreimage {
                        version: 1,
                        features: vec![1, 2],
                        commitment: Default::default(),
                        rangeproof_hash: Default::default(),
                        script: vec![3],
                        sender_offset_public_key: Default::default(),
                        metadata_signature: Default::default(),
                        covenant: vec![0],
                        encrypted_data: vec![9; 10],
                        minimum_value_promise: 0,
                    },
                    normal_output_proof: empty_mmr_proof(),
                    normal_output_mr: Default::default(),
                    block_output_proof: empty_mmr_proof(),
                },
                value: 1000,
            },
            encrypted_data: vec![1, 2, 3],
            mined_in_epoch: 7,
        };

        write_claim_burn_proof_file(&nested, &proof).await.unwrap();
        assert!(nested.exists());
        assert!(!nested.with_extension("json.tmp").exists());
        let decoded: CompleteClaimBurnProof =
            serde_json::from_slice(&tokio::fs::read(&nested).await.unwrap()).unwrap();
        assert_eq!(decoded.claim_proof.value, 1000);
        assert_eq!(decoded.mined_in_epoch, 7);
    }
}
