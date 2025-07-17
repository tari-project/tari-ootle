//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use minotari_node_grpc_client::grpc;
use minotari_wallet_grpc_client::WalletGrpcClient;
use serde::Serialize;
use tari_core::transactions::transaction_components::payment_id::{PaymentId, TxType};
use tari_crypto::tari_utilities::ByteArray;
use tari_template_lib_types::crypto::{
    CommitmentSignatureBytes,
    PedersenCommitmentBytes,
    RistrettoPublicKeyBytes,
    Scalar32Bytes,
};
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
            payment_id: PaymentId::Open {
                user_data: "Burn funds in swarm".as_bytes().to_vec(),
                tx_type: TxType::Burn,
            }
            .to_bytes(),
            claim_public_key: claim_public_key.to_vec(),
            sidechain_deployment_key: vec![],
        };
        let resp = client.create_burn_transaction(request).await?;
        let resp = resp.into_inner();
        if !resp.is_success {
            return Err(anyhow!("Failed to burn funds: {}", resp.failure_message));
        }

        let ownership_proof = resp
            .ownership_proof
            .ok_or_else(|| anyhow!("No ownership proof in response"))?;
        let commitment = PedersenCommitmentBytes::from_bytes(&resp.commitment)
            .map_err(|e| anyhow!("commitment parse error: {e}"))?;

        let ownership_proof = CommitmentSignatureBytes::new(
            PedersenCommitmentBytes::from_bytes(&ownership_proof.public_nonce)
                .map_err(|e| anyhow!("comsig public_nonce parse error {e}"))?,
            Scalar32Bytes::from_bytes(&ownership_proof.u).map_err(|e| anyhow!("comsig u parse error {e}"))?,
            Scalar32Bytes::from_bytes(&ownership_proof.v).map_err(|e| anyhow!("comsig v parse error {e}"))?,
        );

        let reciprocal_claim_public_key = RistrettoPublicKeyBytes::from_bytes(&resp.reciprocal_claim_public_key)
            .map_err(|e| anyhow!("reciprocal_claim_public_key parse error {e}"))?;

        let proof = BurnClaimProofResponse {
            tx_id: resp.transaction_id,
            claim_public_key,
            claim_proof: ClaimBurnProof {
                commitment,
                ownership_proof,
                reciprocal_claim_public_key,
                range_proof: resp.range_proof,
            },
        };

        Ok(proof)
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
    pub claim_proof: ClaimBurnProof,
}
