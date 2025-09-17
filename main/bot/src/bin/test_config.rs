# Configuration for the Test Runner
# Define multiple scenarios to test the bot under different market conditions.

solana_test_validator_path = "solana-test-validator"
market_simulator_path = "market_simulator" # `cargo run --bin market_simulator`
sniper_bot_path = "sniffer_bot_light"      # `cargo run --bin sniffer_bot_light`

[[scenarios]]
name = "Standard Market Conditions"
duration_secs = 120
token_profile_weights = { Gem = 1, Rug = 9, Trash = 90 }
activity_model = "HypeCycle"

[[scenarios]]
name = "High-Velocity Gem Rush (Stress Test)"
duration_secs = 60
# In this scenario, we want to test the bot's speed and efficiency
token_profile_weights = { Gem = 80, Rug = 10, Trash = 10 } 
activity_model = "HypeCycle"

[[scenarios]]
name = "Rug Pull Minefield (Resilience Test)"
duration_secs = 180
# Test how the bot handles a market flooded with rug pulls
token_profile_weights = { Gem = 5, Rug = 80, Trash = 15 }
activity_model = "HypeCycle"
