use serde::{Deserialize, Serialize};
use std::fs;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SnifferMode {
    Mock,
    Real,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum BroadcastMode {
    /// Strict 1:1 pairing (original behavior)
    Pairwise,
    /// Replicate best transaction to all endpoints
    Replicate,
    /// Round-robin transactions across endpoints
    RoundRobin,
    /// Full fanout - send all transactions to all endpoints
    FullFanout,
}

impl Default for SnifferMode {
    fn default() -> Self {
        SnifferMode::Mock
    }
}


impl Default for BroadcastMode {
    fn default() -> Self {
        BroadcastMode::Pairwise
    }
}


#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    // Endpoints
    #[serde(default = "default_rpc_endpoints")]
    pub rpc_endpoints: Vec<String>,
    /// Optional dedicated WebSocket endpoints for REAL sniffer (logsSubscribe).
    #[serde(default)]
    pub rpc_wss_endpoints: Vec<String>,

    // Keys and engine
    #[serde(default)]
    pub keypair_path: Option<String>,
    #[serde(default = "default_nonce_count")]
    pub nonce_count: usize,
    #[serde(default = "default_gui_interval")]
    pub gui_update_interval_ms: u64,

    // Mode
    #[serde(default)]
    pub sniffer_mode: SnifferMode,
    
    // Broadcast configuration
    #[serde(default)]
    pub broadcast_mode: BroadcastMode,
    #[serde(default = "default_rpc_timeout_sec")]
    pub rpc_timeout_sec: u64,
    #[serde(default = "default_early_cancel_threshold")]
    pub early_cancel_threshold: usize,

    // Metadata fetch (Iteration 9)
    #[serde(default)]
    pub meta_fetch_enabled: bool,
    #[serde(default)]
    pub meta_fetch_commitment: Option<String>,

    // WSS watchdog + reconnect (Iteration 10)
    #[serde(default = "default_wss_required")]
    pub wss_required: bool,
    #[serde(default = "default_wss_heartbeat_ms")]
    pub wss_heartbeat_ms: u64,
    #[serde(default = "default_wss_reconnect_backoff_ms")]
    pub wss_reconnect_backoff_ms: u64,
    #[serde(default = "default_wss_reconnect_backoff_max_ms")]
    pub wss_reconnect_backoff_max_ms: u64,
    #[serde(default = "default_wss_max_silent_ms")]
    pub wss_max_silent_ms: u64,

    // HTTP fallback poller
    #[serde(default = "default_http_fallback_enabled")]
    pub http_fallback_enabled: bool,
    #[serde(default = "default_http_poll_interval_ms")]
    pub http_poll_interval_ms: u64,
    #[serde(default = "default_http_sig_depth")]
    pub http_sig_depth: usize,
    #[serde(default = "default_http_max_parallel_tx_fetch")]
    pub http_max_parallel_tx_fetch: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            rpc_endpoints: default_rpc_endpoints(),
            rpc_wss_endpoints: Vec::new(),
            keypair_path: None,
            nonce_count: default_nonce_count(),
            gui_update_interval_ms: default_gui_interval(),
            sniffer_mode: SnifferMode::Mock,
            broadcast_mode: BroadcastMode::Pairwise,
            rpc_timeout_sec: default_rpc_timeout_sec(),
            early_cancel_threshold: default_early_cancel_threshold(),

            meta_fetch_enabled: false,
            meta_fetch_commitment: Some("confirmed".to_string()),
            wss_required: false,
            wss_heartbeat_ms: default_wss_heartbeat_ms(),
            wss_reconnect_backoff_ms: default_wss_reconnect_backoff_ms(),
            wss_reconnect_backoff_max_ms: default_wss_reconnect_backoff_max_ms(),
            wss_max_silent_ms: default_wss_max_silent_ms(),
            http_fallback_enabled: true,
            http_poll_interval_ms: default_http_poll_interval_ms(),
            http_sig_depth: default_http_sig_depth(),
            http_max_parallel_tx_fetch: default_http_max_parallel_tx_fetch(),
        }
    }
}

fn default_rpc_endpoints() -> Vec<String> {
    vec![
        "https://api.mainnet-beta.solana.com".to_string(),
        "https://solana-api.projectserum.com".to_string(),
    ]
}
fn default_nonce_count() -> usize {
    5
}
fn default_gui_interval() -> u64 {
    200
}
fn default_rpc_timeout_secs() -> u64 {
    8
}
fn default_max_retries() -> u32 {
    3
}

// WSS defaults
fn default_wss_required() -> bool {
    false
}
fn default_wss_heartbeat_ms() -> u64 {
    1_500
}
fn default_wss_reconnect_backoff_ms() -> u64 {
    500
}
fn default_wss_reconnect_backoff_max_ms() -> u64 {
    10_000
}
fn default_wss_max_silent_ms() -> u64 {
    5_000
}

// HTTP fallback defaults
fn default_http_fallback_enabled() -> bool {
    true
}
fn default_http_poll_interval_ms() -> u64 {
    1_000
}
fn default_http_sig_depth() -> usize {
    50
}
fn default_http_max_parallel_tx_fetch() -> usize {
    6
}

// RPC Broadcasting defaults  
fn default_rpc_timeout_sec() -> u64 {
    8
}
fn default_early_cancel_threshold() -> usize {
    2
}

impl Config {
    /// Load configuration from "config.toml" if present, otherwise return defaults.
    /// Applies ENV override with highest priority for sniffer mode:
    /// - SNIFFER_MODE=mock | real
    pub fn load() -> Self {
        let mut cfg = match fs::read_to_string("config.toml") {
            Ok(s) => toml::from_str::<Config>(&s).unwrap_or_default(),
            Err(_) => Config::default(),
        };

        // ENV override has priority
        if let Ok(v) = std::env::var("SNIFFER_MODE") {
            match v.to_lowercase().as_str() {
                "mock" => cfg.sniffer_mode = SnifferMode::Mock,
                "real" => cfg.sniffer_mode = SnifferMode::Real,
                _ => { /* ignore invalid value */ }
            }
        }

        cfg.validate().expect("Invalid configuration");
        cfg
    }

    /// Validate configuration consistency and constraints
    pub fn validate(&self) -> Result<(), String> {
        if self.nonce_count == 0 {
            return Err("nonce_count must be greater than 0".to_string());
        }
        
        if self.gui_update_interval_ms == 0 {
            return Err("gui_update_interval_ms must be greater than 0".to_string());
        }
        
        if self.wss_heartbeat_ms == 0 {
            return Err("wss_heartbeat_ms must be greater than 0".to_string());
        }
        
        if self.wss_reconnect_backoff_ms == 0 {
            return Err("wss_reconnect_backoff_ms must be greater than 0".to_string());
        }
        
        if self.wss_reconnect_backoff_max_ms == 0 {
            return Err("wss_reconnect_backoff_max_ms must be greater than 0".to_string());
        }
        
        if self.wss_max_silent_ms == 0 {
            return Err("wss_max_silent_ms must be greater than 0".to_string());
        }
        
        if self.http_poll_interval_ms == 0 {
            return Err("http_poll_interval_ms must be greater than 0".to_string());
        }
        
        if self.wss_reconnect_backoff_ms > self.wss_reconnect_backoff_max_ms {
            return Err("wss_reconnect_backoff_ms cannot be greater than wss_reconnect_backoff_max_ms".to_string());
        }
        
        if self.rpc_endpoints.is_empty() {
            return Err("At least one RPC endpoint must be configured".to_string());
        }
        
        Ok(())
    }
}