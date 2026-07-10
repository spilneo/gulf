use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteRequest {
    pub from_chain: String,
    pub to_chain: String,
    pub from_token: String,
    pub to_token: String,
    pub amount: String,
    pub sender: String,
    pub recipient: Option<String>,
    #[serde(default)]
    pub live: bool,
    pub partner_fee: Option<PartnerFeeRequest>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PartnerFeeRequest {
    pub partner_id: String,
    pub fee_bps: Option<f64>,
    pub recipient: Option<String>,
    #[serde(default)]
    pub metadata: BTreeMap<String, String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PrepareSwapRequest {
    pub quote: QuoteRequest,
    pub provider_id: Option<String>,
}

/// A request to prepare, locally sign, and submit EVM actions. The private key
/// is deliberately not a field: it is read only from `private_key_env` at the
/// point of local signing and is never serialized.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteSwapRequest {
    pub quote: QuoteRequest,
    pub provider_id: Option<String>,
    pub private_key_env: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionReceipt {
    pub transaction_hash: String,
    pub chain: String,
    pub block_number: u64,
    pub status: String,
    pub action_type: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecuteSwapResult {
    pub receipts: Vec<ExecutionReceipt>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuoteResult {
    pub provider_id: String,
    pub provider_name: String,
    pub estimated_fee_bps: f64,
    pub commission_bps: f64,
    pub protocol_fee_bps: f64,
    pub owner_fee_bps: f64,
    pub partner_fee_bps: f64,
    pub integrator_fee_bps: f64,
    pub total_configured_bps: f64,
    pub dynamic_fee: bool,
    pub settlement_seconds: Option<u64>,
    pub docs: Vec<String>,
    pub normalized: Option<NormalizedQuote>,
    pub live_response: Option<serde_json::Value>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedQuote {
    pub output_amount: Option<String>,
    pub estimated_fee_bps: Option<f64>,
    pub estimated_fee_usd: Option<f64>,
    pub gas_fee_usd: Option<f64>,
    pub settlement_seconds: Option<u64>,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PreparedSwap {
    pub provider_id: String,
    pub provider_name: String,
    pub request: QuoteRequest,
    pub normalized_transaction: Option<NormalizedTransaction>,
    pub execution: ExecutionPlan,
    pub transaction: serde_json::Value,
    pub raw: serde_json::Value,
    pub docs: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NormalizedTransaction {
    pub to: Option<String>,
    pub data: Option<String>,
    pub value: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionPlan {
    pub kind: ExecutionKind,
    pub actions: Vec<ExecutionAction>,
    pub status: Option<StatusHandle>,
    pub expires_at: Option<String>,
    pub warnings: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ExecutionKind {
    EvmTransaction,
    EvmApprovalThenTransaction,
    MultiStep,
    NativeDeposit,
    NativeMemoDeposit,
    SolanaTransaction,
    TronTransaction,
    IntentOrder,
    CentralizedDeposit,
    EvmApprovalOnly,
    SerializedTransaction,
    TypedSignatureRequest,
    Unsupported,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ExecutionAction {
    pub chain: Option<String>,
    pub action_type: Option<String>,
    pub description: Option<String>,
    pub to: Option<String>,
    pub data: Option<String>,
    pub value: Option<String>,
    pub token: Option<String>,
    pub spender: Option<String>,
    pub amount: Option<String>,
    pub deposit_address: Option<String>,
    pub memo: Option<String>,
    pub serialized_transaction: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub typed_data: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub intent: Option<serde_json::Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub submission: Option<serde_json::Value>,
    pub raw: serde_json::Value,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StatusHandle {
    pub provider_id: String,
    pub status_endpoint: Option<String>,
    pub id: Option<String>,
    pub tx_hash: Option<String>,
}
