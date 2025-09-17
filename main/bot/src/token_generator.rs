//! Shared types for TokenGenerator
//! 
//! This module contains the shared types used by the TokenGenerator binary
//! and other parts of the system that need to interact with generated tokens.

use solana_sdk::pubkey::Pubkey;
use fastrand;

/// Token profile types with associated probabilities
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TokenProfile {
    /// High-quality token with real metadata and significant liquidity (1% probability)
    Gem,
    /// Rug pull token with minimal liquidity, may disappear quickly (9% probability)
    Rug,
    /// Low-quality trash token with poor metadata (90% probability)
    Trash,
}

impl TokenProfile {
    /// Get the probability weight for this profile
    pub fn weight(&self) -> u32 {
        match self {
            TokenProfile::Gem => 1,
            TokenProfile::Rug => 9,
            TokenProfile::Trash => 90,
        }
    }

    /// Get a description of this profile
    pub fn description(&self) -> &'static str {
        match self {
            TokenProfile::Gem => "High-quality gem with strong fundamentals",
            TokenProfile::Rug => "Rug pull candidate - will disappear soon",
            TokenProfile::Trash => "Low-quality trash token with minimal activity",
        }
    }

    /// Generate a random profile based on weighted probabilities
    pub fn random() -> Self {
        let rand = fastrand::u32(1..=100);
        match rand {
            1 => TokenProfile::Gem,
            2..=10 => TokenProfile::Rug,
            _ => TokenProfile::Trash,
        }
    }
}

/// Represents a generated token with its metadata
#[derive(Debug, Clone)]
pub struct GeneratedToken {
    /// The mint pubkey of the generated token
    pub mint: Pubkey,
    /// The profile type of this token
    pub profile: TokenProfile,
    /// The creator wallet pubkey
    pub creator: Pubkey,
    /// Timestamp when this token was created
    pub created_at: u64,
    /// Initial supply that was minted
    pub initial_supply: u64,
    /// Amount of liquidity added (in lamports)
    pub liquidity_lamports: u64,
    /// Metadata URI (if any)
    pub metadata_uri: Option<String>,
}