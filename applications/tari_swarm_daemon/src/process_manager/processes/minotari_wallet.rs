//   Copyright 2024 The Tari Project
//   SPDX-License-Identifier: BSD-3-Clause

use anyhow::anyhow;
use base64::{engine::general_purpose::STANDARD as BASE64, Engine};
use minotari_node_grpc_client::grpc;
use minotari_wallet_grpc_client::WalletGrpcClient;
use serde::Serialize;
use serde_json::json;
use tari_common_types::types::PrivateKey;
use tari_core::transactions::transaction_components::encrypted_data::{PaymentId, TxType};
use tari_crypto::{
    ristretto::{pedersen::PedersenCommitment, RistrettoComSig, RistrettoPublicKey},
    tari_utilities::ByteArray,
};
use tari_template_lib_types::crypto::RistrettoPublicKeyBytes;

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
    ) -> anyhow::Result<BurnClaimProofJson> {
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
        let commitment = PedersenCommitment::from_canonical_bytes(&resp.commitment)
            .map_err(|e| anyhow!("commitment parse error: {e}"))?;

        let ownership_proof = RistrettoComSig::new(
            PedersenCommitment::from_canonical_bytes(&ownership_proof.public_nonce)
                .map_err(|e| anyhow!("comsig public_nonce parse error {e}"))?,
            PrivateKey::from_canonical_bytes(&ownership_proof.u).map_err(|e| anyhow!("comsig u parse error {e}"))?,
            PrivateKey::from_canonical_bytes(&ownership_proof.v).map_err(|e| anyhow!("comsig v parse error {e}"))?,
        );

        let reciprocal_claim_public_key = RistrettoPublicKey::from_canonical_bytes(&resp.reciprocal_claim_public_key)
            .map_err(|e| anyhow!("reciprocal_claim_public_key parse error {e}"))?;

        let proof = BurnClaimProofJson {
            tx_id: resp.transaction_id,
            claim_public_key,
            claim_proof: json!({
                "commitment": BASE64.encode(commitment.as_bytes()),
                "ownership_proof": {
                    "public_nonce": BASE64.encode(ownership_proof.public_nonce().as_bytes()),
                    "u": BASE64.encode(ownership_proof.u().as_bytes()),
                    "v": BASE64.encode(ownership_proof.v().as_bytes())
                },
                "reciprocal_claim_public_key": BASE64.encode(reciprocal_claim_public_key.as_bytes()),
                "range_proof": BASE64.encode(&resp.range_proof),
            }),
        };

        Ok(proof)
    }

    pub async fn get_balance(&self) -> anyhow::Result<grpc::GetBalanceResponse> {
        let mut client = self.connect_client().await?;
        let balance = client.get_balance(grpc::GetBalanceRequest {}).await?.into_inner();
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
pub struct BurnClaimProofJson {
    pub tx_id: u64,
    pub claim_public_key: RistrettoPublicKeyBytes,
    pub claim_proof: serde_json::Value,
}
