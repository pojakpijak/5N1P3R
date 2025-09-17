/*!
Test Environment Runner for SNIPER Trading Bot (Production Grade v4)

This program orchestrates a comprehensive, scenario-driven test environment.
Key features:
- Builds and runs simulator/bot as release binaries.
- Streams stdout/stderr to log files to conserve memory (`.jsonl` for structured logs).
- Parses structured JSON logs for precise, reliable metrics.
- Calculates accurate Time-to-Execute (TTE) using embedded timestamps.
- Categorizes errors from stderr for better analysis.
- Exports a final performance report to a JSON file.
- Uses `clap` for robust command-line argument parsing.

Usage:
    cargo run --bin test_runner -- --scenarios <PATH> [--output <PATH>]
*/

use anyhow::{anyhow, Context, Result};
use chrono::Utc;
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{error, info};
use tracing_subscriber::EnvFilter;

// --- Command-Line Argument Parsing ---

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Cli {
    /// Path to the scenarios configuration TOML file.
    #[arg(long, default_value = "MarketSimulator/scenarios.toml")]
    scenarios: PathBuf,

    /// Path to export the final JSON results file.
    #[arg(long, default_value = "test_results.json")]
    output: PathBuf,
}

// --- Configuration Structs ---

#[derive(Debug, Deserialize, Clone)]
struct TestScenario {
    name: String,
    duration_secs: u64,
}

#[derive(Debug, Deserialize, Clone)]
struct TestConfig {
    scenarios: Vec<TestScenario>,
    solana_test_validator_path: String,
    market_simulator_crate_path: String,
    sniper_bot_crate_path: String,
}

// --- Log Parsing Structs ---

#[derive(Debug, Deserialize)]
struct LogEntry {
    fields: LogFields,
    target: String,
}

#[derive(Debug, Deserialize)]
struct LogFields {
    message: String,
    mint: Option<String>,
    profile: Option<String>,
    #[serde(rename = "timestamp_ms")]
    timestamp_ms: Option<u128>,
}

// --- Result Aggregation Structs ---

#[derive(Debug, Serialize, Clone, PartialEq, Eq)]
enum ErrorCategory {
    Panic,
    BuildFailed,
    ProcessExited,
    Other,
}

#[derive(Debug, Serialize)]
struct CategorizedError {
    category: ErrorCategory,
    details: String,
}

#[derive(Debug, Serialize)]
struct TestResult {
    scenario_name: String,
    run_started_at: String,
    tokens_generated: HashMap<String, u32>,
    rug_pulls_executed: u32,
    bot_buy_attempts: u32,
    bot_buy_successes: u32,
    success_rate_percent: f64,
    average_tte_ms: f64,
    p95_tte_ms: f64,
    errors: Vec<CategorizedError>,
}

impl TestResult {
    fn new(scenario_name: &str) -> Self {
        Self {
            scenario_name: scenario_name.to_string(),
            run_started_at: Utc::now().to_rfc3339(),
            tokens_generated: HashMap::new(),
            rug_pulls_executed: 0,
            bot_buy_attempts: 0,
            bot_buy_successes: 0,
            success_rate_percent: 0.0,
            average_tte_ms: 0.0,
            p95_tte_ms: 0.0,
            errors: Vec::new(),
        }
    }

    fn calculate_metrics(&mut self, tte_samples: &[u64]) {
        self.success_rate_percent = if self.bot_buy_attempts > 0 {
            (self.bot_buy_successes as f64 / self.bot_buy_attempts as f64) * 100.0
        } else {
            0.0
        };

        if !tte_samples.is_empty() {
            let sum: u64 = tte_samples.iter().sum();
            self.average_tte_ms = sum as f64 / tte_samples.len() as f64;

            let mut sorted_samples = tte_samples.to_vec();
            sorted_samples.sort_unstable();
            let p95_index = ((sorted_samples.len() as f64 * 0.95).floor() as usize).saturating_sub(1);
            self.p95_tte_ms = sorted_samples[p95_index.min(sorted_samples.len() - 1)] as f64;
        }
    }

    fn print_summary(&self) {
        println!("\n--- Test Scenario Summary: '{}' ---", self.scenario_name);
        println!("  Tokens Generated:");
        for (profile, count) in &self.tokens_generated {
            println!("    - {:<8}: {}", profile, count);
        }
        println!("  Simulator Rug Pulls Executed: {}", self.rug_pulls_executed);
        println!("  Bot Buy Attempts: {}", self.bot_buy_attempts);
        println!("  Bot Buy Successes: {}", self.bot_buy_successes);
        println!("  Bot Success Rate: {:.2}%", self.success_rate_percent);
        println!("  Average Time-to-Execute (TTE): {:.2}ms", self.average_tte_ms);
        println!("  P95 Time-to-Execute (TTE): {:.2}ms", self.p95_tte_ms);
        if !self.errors.is_empty() {
            println!("  Errors Encountered: {}", self.errors.len());
            for err in self.errors.iter().take(5) {
                println!("    - [{}]: {}", format!("{:?}", err.category).to_uppercase(), err.details);
            }
        }
        println!("---------------------------------------------------\n");
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_env_filter(EnvFilter::from_default_env()).init();
    let cli = Cli::parse();
    info!("🚀 Starting SNIPER Bot Test Runner");
    info!("Loading scenarios from: {}", cli.scenarios.display());

    let config: TestConfig = toml::from_str(&fs::read_to_string(&cli.scenarios)?)?;
    let mut all_results = Vec::new();

    build_crate("Market Simulator", &config.market_simulator_crate_path)?;
    build_crate("Sniper Bot", &config.sniper_bot_crate_path)?;

    for scenario in config.scenarios {
        info!("--- Running Scenario: {} ---", scenario.name);

        let mut validator = start_validator(&config.solana_test_validator_path)?;
        tokio::time::sleep(Duration::from_secs(10)).await;

        let simulator_bin = PathBuf::from(&config.market_simulator_crate_path).join("target/release/market_simulator");
        let bot_bin = PathBuf::from(&config.sniper_bot_crate_path).join("target/release/sniper-bot");

        let mut simulator = start_process("Market Simulator", &simulator_bin)?;
        let mut bot = start_process("Sniper Bot", &bot_bin)?;

        let mut result = TestResult::new(&scenario.name);

        let (sim_log_path, sim_err_path) = create_log_files("simulator", &scenario.name);
        let (bot_log_path, bot_err_path) = create_log_files("bot", &scenario.name);

        let sim_handle = stream_output_to_file(simulator.stdout.take().unwrap(), &sim_log_path);
        let sim_err_handle = stream_output_to_file(simulator.stderr.take().unwrap(), &sim_err_path);
        let bot_handle = stream_output_to_file(bot.stdout.take().unwrap(), &bot_log_path);
        let bot_err_handle = stream_output_to_file(bot.stderr.take().unwrap(), &bot_err_path);

        let start_time = Instant::now();
        while start_time.elapsed() < Duration::from_secs(scenario.duration_secs) {
            if check_process_exit("Sniper Bot", &mut bot, &mut result)?
                || check_process_exit("Market Simulator", &mut simulator, &mut result)?
            {
                break;
            }
            tokio::time::sleep(Duration::from_secs(1)).await;
        }

        info!("Scenario duration elapsed. Stopping processes...");
        let _ = simulator.kill();
        let _ = bot.kill();
        let _ = validator.kill();

        // Wait for log streaming to finish
        let _ = sim_handle.join();
        let _ = sim_err_handle.join();
        let _ = bot_handle.join();
        let _ = bot_err_handle.join();

        parse_logs(&mut result, &sim_log_path, &bot_log_path)?;
        result.errors.extend(parse_errors(&sim_err_path));
        result.errors.extend(parse_errors(&bot_err_path));

        result.print_summary();
        all_results.push(result);
    }

    fs::write(&cli.output, serde_json::to_string_pretty(&all_results)?)?;
    info!("📈 Test results exported to {}", cli.output.display());

    Ok(())
}

fn build_crate(name: &str, path: &str) -> Result<()> {
    info!("Building {}...", name);
    let status = Command::new("cargo").arg("build").arg("--release").current_dir(path).status()?;
    if !status.success() {
        return Err(anyhow!("Failed to build {}", name));
    }
    Ok(())
}

fn start_validator(path: &str) -> Result<Child> {
    info!("Starting solana-test-validator...");
    Command::new(path)
        .args(["--reset", "--quiet", "--limit-ledger-size", "50000000"])
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context("Failed to start solana-test-validator")
}

fn start_process(name: &str, path: &Path) -> Result<Child> {
    info!("Starting {} from binary: {}", name, path.display());
    Command::new(path)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .context(format!("Failed to start {}", name))
}

fn check_process_exit(name: &str, process: &mut Child, result: &mut TestResult) -> Result<bool> {
    if let Some(status) = process.try_wait()? {
        if !status.success() {
            let err_msg = format!("{} exited prematurely with status: {}", name, status);
            error!("{}", err_msg);
            result.errors.push(CategorizedError {
                category: ErrorCategory::ProcessExited,
                details: err_msg,
            });
            return Ok(true);
        }
    }
    Ok(false)
}

fn create_log_files(prefix: &str, scenario_name: &str) -> (PathBuf, PathBuf) {
    let sanitized_name = scenario_name.replace(|c: char| !c.is_alphanumeric(), "_").to_lowercase();
    let stdout_path = PathBuf::from(format!("{}_{}.stdout.jsonl", prefix, sanitized_name));
    let stderr_path = PathBuf::from(format!("{}_{}.stderr.log", prefix, sanitized_name));
    (stdout_path, stderr_path)
}

fn stream_output_to_file<R>(stream: R, path: &Path) -> thread::JoinHandle<()>
where
    R: std::io::Read + Send + 'static,
{
    let path_buf = path.to_path_buf();
    thread::spawn(move || {
        let mut file = File::create(path_buf).expect("Failed to create log file");
        let reader = BufReader::new(stream);
        for (i, line) in reader.lines().flatten().enumerate() {
            if writeln!(file, "{}", line).is_err() {
                break;
            }
            // Flush every 10 lines to prevent data loss on crash without too much IO overhead
            if i % 10 == 0 {
                let _ = file.flush();
            }
        }
    })
}

fn parse_logs(result: &mut TestResult, sim_log_path: &Path, bot_log_path: &Path) -> Result<()> {
    let mut creation_times: HashMap<String, u128> = HashMap::new();
    let mut tte_samples = Vec::new();

    let sim_file = File::open(sim_log_path)?;
    for line in BufReader::new(sim_file).lines().flatten() {
        if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
            if entry.target == "token_generator" && entry.fields.message.contains("Generated token") {
                if let (Some(mint), Some(profile), Some(ts)) = (entry.fields.mint, entry.fields.profile, entry.fields.timestamp_ms) {
                    creation_times.insert(mint, ts);
                    *result.tokens_generated.entry(profile).or_insert(0) += 1;
                }
            } else if entry.target == "market_maker" && entry.fields.message.contains("Executing RUG PULL") {
                result.rug_pulls_executed += 1;
            }
        }
    }

    let bot_file = File::open(bot_log_path)?;
    for line in BufReader::new(bot_file).lines().flatten() {
        if let Ok(entry) = serde_json::from_str::<LogEntry>(&line) {
            if entry.target == "engine" && entry.fields.message.contains("Handling BUY for candidate") {
                result.bot_buy_attempts += 1;
            } else if entry.target == "engine" && entry.fields.message.contains("Quantum Race BUY successful") {
                result.bot_buy_successes += 1;
                if let (Some(mint), Some(ts)) = (entry.fields.mint, entry.fields.timestamp_ms) {
                    if let Some(start_ts) = creation_times.get(&mint) {
                        if ts > *start_ts {
                            tte_samples.push((ts - start_ts) as u64);
                        }
                    }
                }
            }
        }
    }

    result.calculate_metrics(&tte_samples);
    Ok(())
}

fn parse_errors(err_log_path: &Path) -> Vec<CategorizedError> {
    let mut errors = Vec::new();
    if let Ok(file) = File::open(err_log_path) {
        for line in BufReader::new(file).lines().flatten() {
            let category = if line.contains("panicked at") {
                ErrorCategory::Panic
            } else if line.contains("failed to build") {
                ErrorCategory::BuildFailed
            } else {
                ErrorCategory::Other
            };
            errors.push(CategorizedError {
                category,
                details: line,
            });
        }
    }
    errors
}