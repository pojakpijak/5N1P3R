//! Test to demonstrate Pillar II optimization when performance is poor

use anyhow::Result;
use h_5n1p3r::oracle::{
    DecisionLedger, TransactionRecord, Outcome, ScoredCandidate,
    PerformanceMonitor, StrategyOptimizer, FeatureWeights, ScoreThresholds,
};
use h_5n1p3r::types::PremintCandidate;
use std::collections::HashMap;
use tokio::sync::mpsc;
use tracing::{info, Level};
use tracing_subscriber;

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize logging
    tracing_subscriber::fmt()
        .with_max_level(Level::INFO)
        .init();

    info!("Testing Pillar II optimization with poor performance data");

    // Create channels
    let (decision_record_sender, decision_record_receiver) = mpsc::channel(100);
    let (outcome_update_sender, outcome_update_receiver) = mpsc::channel(100);
    let (perf_report_sender, perf_report_receiver) = mpsc::channel(16);
    let (opt_params_sender, mut opt_params_receiver) = mpsc::channel(16);

    // Initialize DecisionLedger
    let decision_ledger = DecisionLedger::new(
        decision_record_receiver,
        outcome_update_receiver,
    ).await?;

    let db_pool = decision_ledger.get_db_pool().clone();

    // Start DecisionLedger
    let _ledger_handle = tokio::spawn(async move {
        decision_ledger.run().await;
    });

    // Add several losing trades with specific feature patterns
    create_poor_performance_data(decision_record_sender).await?;

    // Give data time to be recorded
    tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;

    // Manually trigger performance analysis and optimization
    let performance_monitor = PerformanceMonitor::new(
        db_pool.clone(),
        perf_report_sender.clone(),
        999, // Not used in manual test
        24,
    );

    // Run analysis manually and send poor performance report
    tokio::spawn(async move {
        // Wait a bit for data to be recorded
        tokio::time::sleep(tokio::time::Duration::from_millis(200)).await;
        
        if let Ok(report) = performance_monitor.analyze_performance().await {
            info!("Performance analysis results: PF={:.2}, WR={:.1}%", 
                  report.profit_factor, report.win_rate_percent);
            
            // If performance is not poor enough, create a poor report manually
            let poor_report = if report.profit_factor >= 1.2 {
                use h_5n1p3r::oracle::PerformanceReport;
                PerformanceReport {
                    timestamp: chrono::Utc::now().timestamp_millis() as u64,
                    time_window_hours: 24.0,
                    total_trades_evaluated: 15,
                    win_rate_percent: 20.0, // Poor win rate
                    profit_factor: 0.6,     // Poor profit factor
                    average_profit_sol: 0.1,
                    average_loss_sol: 0.4,
                    net_profit_sol: -1.5,   // Net loss
                    max_drawdown_percent: 15.0,
                }
            } else {
                report
            };
            
            let _ = perf_report_sender.send(poor_report).await;
        }
    });

    // Initialize StrategyOptimizer
    let strategy_optimizer = StrategyOptimizer::new(
        db_pool,
        perf_report_receiver,
        opt_params_sender,
        FeatureWeights::default(),
        ScoreThresholds::default(),
    );

    let _opt_handle = tokio::spawn(async move {
        strategy_optimizer.run().await;
    });

    // Wait for optimization results
    if let Some(optimized_params) = opt_params_receiver.recv().await {
        info!("🎯 Optimization successful!");
        info!("Reason: {}", optimized_params.reason);
        info!("Original liquidity weight: {:.3}", FeatureWeights::default().liquidity);
        info!("New liquidity weight: {:.3}", optimized_params.new_weights.liquidity);
        info!("Improvement: {:.1}%", 
              ((optimized_params.new_weights.liquidity / FeatureWeights::default().liquidity) - 1.0) * 100.0);
    } else {
        info!("No optimization parameters received");
    }

    Ok(())
}

async fn create_poor_performance_data(sender: mpsc::Sender<TransactionRecord>) -> Result<()> {
    info!("Creating poor performance data to trigger optimization...");

    // Create several losing trades with consistently low liquidity scores
    for i in 1..=5 {
        let mut feature_scores = HashMap::new();
        // Low liquidity scores (the pattern we want to optimize)
        feature_scores.insert("liquidity".to_string(), 0.1 + (i as f64 * 0.05));
        feature_scores.insert("holder_distribution".to_string(), 0.7);
        feature_scores.insert("volume_growth".to_string(), 0.8);

        let candidate = PremintCandidate {
            mint: format!("LossToken{}Address", i),
            creator: format!("Creator{}Address", i),
            program: "pump.fun".to_string(),
            slot: 50000 + i,
            timestamp: chrono::Utc::now().timestamp_millis() as u64,
            instruction_summary: Some("Losing token".to_string()),
            is_jito_bundle: Some(false),
        };

        let scored_candidate = ScoredCandidate {
            base: candidate.clone(),
            mint: candidate.mint.clone(),
            predicted_score: 65, // Mediocre score
            reason: format!("Token with low liquidity #{}", i),
            feature_scores: feature_scores.clone(),
            calculation_time: 120_000,
            anomaly_detected: false,
            timestamp: candidate.timestamp,
        };

        // Create a losing transaction record
        let loss_record = TransactionRecord {
            id: None,
            scored_candidate,
            transaction_signature: Some(format!("LossTx{}_{}", i, candidate.timestamp)),
            buy_price_sol: Some(0.001),
            sell_price_sol: Some(0.0005), // Lost half the value
            amount_bought_tokens: Some(1000.0),
            amount_sold_tokens: Some(1000.0),
            initial_sol_spent: Some(1.0),
            final_sol_received: Some(0.5), // 50% loss
            timestamp_decision_made: candidate.timestamp,
            timestamp_transaction_sent: Some(candidate.timestamp + 1000),
            timestamp_outcome_evaluated: Some(candidate.timestamp + 10000),
            actual_outcome: Outcome::Loss(-0.5), // 0.5 SOL loss
            market_context_snapshot: HashMap::new(),
        };

        sender.send(loss_record).await?;
    }

    info!("Created 5 losing trades with low liquidity scores");
    Ok(())
}