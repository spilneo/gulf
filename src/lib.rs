pub mod alloy_support;
pub mod cli;
pub mod client;
pub mod config;
pub mod execution;
pub mod model;
pub mod provider;
pub mod rpc;
pub mod server;

pub use client::Gulf;
pub use config::{GulfConfig, ProviderSummary};
pub use model::{
    ExecuteSwapRequest, ExecuteSwapResult, ExecutionAction, ExecutionKind, ExecutionPlan,
    ExecutionReceipt, NormalizedQuote, NormalizedTransaction, PartnerFeeRequest,
    PrepareSwapRequest, PreparedSwap, QuoteRequest, QuoteResult, StatusHandle,
};
