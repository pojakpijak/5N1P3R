//! Oracle module - DecisionLedger and Pillar II components
//!
//! This module contains the DecisionLedger operational memory system (Pillar I)
//! and the PerformanceMonitor/StrategyOptimizer feedback loop (Pillar II).

pub mod types;
pub mod types_old; // Old types that are still in use
pub mod decision_ledger;
pub mod transaction_monitor;
pub mod performance_monitor;
pub mod strategy_optimizer;
pub mod market_regime_detector; // Pillar III
pub mod data_sources; // For MarketRegimeDetector

// Re-export main types
pub use types::{
    ScoredCandidate, TransactionRecord, Outcome,
    DecisionRecordSender, OutcomeUpdateSender,
    FeatureWeights, ScoreThresholds,
    PerformanceReport, OptimizedParameters,
    PerformanceReportSender, PerformanceReportReceiver,
    OptimizedParametersSender, OptimizedParametersReceiver,
    // Pillar III types
    MarketRegime, RegimeSpecificParameters, OracleConfig,
};

// Re-export key components
pub use decision_ledger::DecisionLedger;
pub use transaction_monitor::{TransactionMonitor, MonitoredTransaction};
pub use performance_monitor::PerformanceMonitor;
pub use strategy_optimizer::StrategyOptimizer;
pub use market_regime_detector::MarketRegimeDetector; // Pillar III
pub use data_sources::OracleDataSources; // For MarketRegimeDetector