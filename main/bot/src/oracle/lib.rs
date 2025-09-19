//! H-5N1P3R - High-performance Solana trading oracle system
//! 
//! This crate provides a universe-class predictive oracle system for Solana token analysis
//! with operational memory (DecisionLedger) capabilities.

pub mod types;
pub mod oracle;

// Re-export main types for convenience
pub use types::{PremintCandidate, QuantumCandidateGui};