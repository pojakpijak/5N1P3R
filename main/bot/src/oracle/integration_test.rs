//! Tests for the DecisionLedger system

use h_5n1p3r::oracle::{
    DecisionLedger, TransactionRecord, Outcome, ScoredCandidate,
};
use h_5n1p3r::types::PremintCandidate;
use std::collections::HashMap;
use tokio::sync::mpsc;

#[tokio::test]
async fn test_decision_ledger_basic_functionality() {
    // Setup channels
    let (decision_sender, decision_receiver) = mpsc::channel(10);
    let (outcome_sender, outcome_receiver) = mpsc::channel(10);

    // Create DecisionLedger
    let ledger = DecisionLedger::new(decision_receiver, outcome_receiver)
        .await
        .expect("Failed to create DecisionLedger");

    // Create a test candidate
    let candidate = PremintCandidate {
        mint: "TestToken123".to_string(),
        creator: "TestCreator123".to_string(),
        program: "test.program".to_string(),
        slot: 12345,
        timestamp: chrono::Utc::now().timestamp_millis() as u64,
        instruction_summary: Some("Test instruction".to_string()),
        is_jito_bundle: Some(true),
    };

    let scored_candidate = ScoredCandidate {
        base: candidate.clone(),
        mint: candidate.mint.clone(),
        predicted_score: 85,
        reason: "Test scoring".to_string(),
        feature_scores: HashMap::new(),
        calculation_time: 100_000,
        anomaly_detected: false,
        timestamp: candidate.timestamp,
    };

    // Create a transaction record
    let record = TransactionRecord {
        id: None,
        scored_candidate,
        transaction_signature: Some("TestSignature123".to_string()),
        buy_price_sol: Some(0.001),
        sell_price_sol: None,
        amount_bought_tokens: Some(1000.0),
        amount_sold_tokens: None,
        initial_sol_spent: Some(1.0),
        final_sol_received: None,
        timestamp_decision_made: candidate.timestamp,
        timestamp_transaction_sent: Some(candidate.timestamp + 1000),
        timestamp_outcome_evaluated: None,
        actual_outcome: Outcome::PendingConfirmation,
        market_context_snapshot: HashMap::new(),
    };

    // Start ledger in background
    let _ledger_handle = tokio::spawn(async move {
        ledger.run().await;
    });

    // Send the record
    decision_sender.send(record).await.expect("Failed to send record");

    // Update the outcome
    outcome_sender.send((
        "TestSignature123".to_string(),
        Outcome::Profit(0.1),
        Some(0.001),
        Some(0.0011),
        Some(1.0),
        Some(1.1),
        Some(candidate.timestamp + 5000),
    )).await.expect("Failed to send outcome update");

    // Give the system a moment to process
    tokio::time::sleep(tokio::time::Duration::from_millis(100)).await;

    // The test passes if no panics occur during the operation
    println!("DecisionLedger test completed successfully!");
}