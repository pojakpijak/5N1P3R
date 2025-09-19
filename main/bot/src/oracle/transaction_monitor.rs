//! TransactionMonitor module - monitors the outcome of sent transactions
//!
//! This module tracks transactions sent by the Oracle and evaluates their final outcomes,
//! updating the DecisionLedger with profit/loss information.

use std::time::Duration;
use tokio::{sync::mpsc, time::sleep};
use tracing::{info, warn, error};
use crate::oracle::types::{Outcome, OutcomeUpdateSender};
use crate::types::Pubkey;
use rand::Rng;

/// Represents a transaction being monitored for outcome
#[derive(Debug, Clone)]
pub struct MonitoredTransaction {
    pub signature: String,
    pub mint: Pubkey,
    pub amount_bought_tokens: f64, // Expected tokens to buy
    pub initial_sol_spent: f64,    // SOL spent on purchase
    pub monitor_until: u64,        // Timestamp until when to monitor (e.g., 30s after sending)
    // Future: could include pool address, creator address, etc. for more detailed monitoring
}

/// TransactionMonitor tracks the outcomes of trading transactions
pub struct TransactionMonitor {
    active_transactions: Vec<MonitoredTransaction>, // Transactions currently being monitored
    update_sender: OutcomeUpdateSender, // Channel to send updates to DecisionLedger
    monitor_interval: Duration,
}

impl TransactionMonitor {
    /// Create a new TransactionMonitor
    pub fn new(
        update_sender: OutcomeUpdateSender,
        monitor_interval_ms: u64,
    ) -> Self {
        Self {
            active_transactions: Vec::new(),
            update_sender,
            monitor_interval: Duration::from_millis(monitor_interval_ms),
        }
    }

    /// Main execution loop - monitors active transactions and processes new ones
    pub async fn run(mut self, mut new_tx_receiver: mpsc::Receiver<MonitoredTransaction>) {
        info!("TransactionMonitor is running...");
        loop {
            tokio::select! {
                // Receive new transactions to monitor
                Some(new_tx) = new_tx_receiver.recv() => {
                    info!("Adding new transaction to monitor: {}", new_tx.signature);
                    self.active_transactions.push(new_tx);
                },
                // Periodically check active transactions
                _ = sleep(self.monitor_interval) => {
                    self.process_active_transactions().await;
                },
                else => {
                    info!("TransactionMonitor channels closed. Shutting down.");
                    break;
                }
            }
        }
    }

    /// Process all currently active transactions
    async fn process_active_transactions(&mut self) {
        let now = chrono::Utc::now().timestamp_millis() as u64;
        let mut transactions_to_remove = Vec::new();

        for (i, tx) in self.active_transactions.iter().enumerate() {
            if tx.monitor_until < now {
                warn!("Monitoring for transaction {} expired. Marking as Neutral (Not Executed).", tx.signature);
                // Send timeout status to DecisionLedger
                if let Err(e) = self.update_sender.send((
                    tx.signature.clone(),
                    Outcome::Neutral,
                    None, None, None, None, Some(now)
                )).await {
                    error!("Failed to send expired outcome update: {}", e);
                }
                transactions_to_remove.push(i);
                continue;
            }

            // --- SIMULATED TRANSACTION OUTCOME EVALUATION ---
            // In a real implementation, this would:
            // 1. Check transaction status via RPC (get_signature_statuses, get_transaction)
            // 2. Parse transaction logs and account data to determine:
            //    a. How much SOL was actually spent
            //    b. How many tokens were actually bought
            //    c. Purchase price
            //    d. Potential sale price (if monitoring sales too)
            // 3. Evaluate profit/loss
            
            let simulated_outcome = self.simulate_transaction_outcome(tx).await;
            
            if let Some((outcome, buy_price_sol, sell_price_sol, final_sol_received)) = simulated_outcome {
                info!("Transaction {} outcome evaluated: {:?}", tx.signature, outcome);
                if let Err(e) = self.update_sender.send((
                    tx.signature.clone(),
                    outcome,
                    Some(buy_price_sol),
                    sell_price_sol,
                    Some(tx.initial_sol_spent),
                    final_sol_received,
                    Some(now)
                )).await {
                    error!("Failed to send outcome update: {}", e);
                }
                transactions_to_remove.push(i); // Remove from monitoring after evaluation
            }
        }

        // Remove completed transactions
        for i in transactions_to_remove.into_iter().rev() {
            self.active_transactions.remove(i);
        }
    }

    /// Simulate transaction outcome evaluation
    /// 
    /// This is a placeholder implementation. In a real system, this would involve:
    /// - Querying Solana RPC for transaction confirmation and details
    /// - Parsing transaction logs to extract swap details
    /// - Calculating actual profit/loss based on buy/sell prices
    /// - Handling failed transactions, timeouts, etc.
    async fn simulate_transaction_outcome(&self, tx: &MonitoredTransaction) -> Option<(Outcome, f64, Option<f64>, Option<f64>)> {
        // Simulation: 50% chance for Profit, 30% for Loss, 20% for FailedExecution
        let mut rng = rand::thread_rng();
        let outcome_roll = rng.gen_range(0.0..1.0);

        if outcome_roll < 0.5 {
            // Simulate profit (5-20% gain)
            let profit = tx.initial_sol_spent * rng.gen_range(0.05..0.20);
            let final_sol_received = tx.initial_sol_spent + profit;
            let buy_price = tx.initial_sol_spent / tx.amount_bought_tokens;
            let sell_price = final_sol_received / tx.amount_bought_tokens;
            Some((Outcome::Profit(profit), buy_price, Some(sell_price), Some(final_sol_received)))
        } else if outcome_roll < 0.8 {
            // Simulate loss (2-10% loss)
            let loss = tx.initial_sol_spent * rng.gen_range(0.02..0.10);
            let final_sol_received = tx.initial_sol_spent - loss;
            let buy_price = tx.initial_sol_spent / tx.amount_bought_tokens;
            let sell_price = final_sol_received / tx.amount_bought_tokens;
            Some((Outcome::Loss(loss), buy_price, Some(sell_price), Some(final_sol_received)))
        } else {
            // Simulate failed execution
            error!("Simulated failed execution for transaction: {}", tx.signature);
            Some((Outcome::FailedExecution, 0.0, None, Some(tx.initial_sol_spent))) // Assume SOL returned
        }
    }

    /// Get a read-only reference to active transactions (for debugging/metrics)
    pub fn get_active_transactions(&self) -> &[MonitoredTransaction] {
        &self.active_transactions
    }
}