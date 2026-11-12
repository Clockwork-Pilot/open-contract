//! Rust core layers for manifest planning, filesystem isolation, batched
//! execution, and verification outside the language adapter.

pub mod batch;
pub mod isolation;
pub mod manifest;
pub mod verify;

pub use batch::{
    run_contract_tree, BatchAdapter, BatchReport, CommandBatchAdapter, ContractBatch,
    ContractResult, RuntimeBatchAdapter,
};
pub use isolation::{Backend, StagedTree};
pub use manifest::{Manifest, ManifestPlan};
