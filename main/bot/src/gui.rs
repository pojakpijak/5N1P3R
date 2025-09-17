use std::sync::Arc;
use std::time::Duration;

use anyhow::Result;
use eframe::egui::{self, Key};
use eframe::{App, Frame};
use tokio::sync::{mpsc::Sender, Mutex};
use tracing::info;

use crate::types::{AppState, Mode, QuantumCandidateGui};
use solana_sdk::pubkey::Pubkey;

#[derive(Clone, Debug)]
pub enum GuiEvent {
    SellPercent(f64),
    Buy(Pubkey), // New buy event for quantum manual mode
}
pub type GuiEventSender = Sender<GuiEvent>;

pub fn launch_gui(
    title: &str,
    app_state: Arc<Mutex<AppState>>,
    gui_tx: GuiEventSender,
    refresh: Duration,
) -> Result<()> {
    let native_options = eframe::NativeOptions::default();
    let app = BotApp::new(app_state, gui_tx, refresh);
    eframe::run_native(title, native_options, Box::new(|_| Box::new(app)))
        .map_err(|e| anyhow::anyhow!("GUI error: {}", e))?;
    Ok(())
}

struct BotApp {
    app_state: Arc<Mutex<AppState>>,
    gui_tx: GuiEventSender,
    refresh: Duration,
}

impl BotApp {
    fn new(app_state: Arc<Mutex<AppState>>, gui_tx: GuiEventSender, refresh: Duration) -> Self {
        Self {
            app_state,
            gui_tx,
            refresh,
        }
    }

    fn draw_state(&self, ui: &mut egui::Ui, st: &AppState) {
        ui.heading("Sniffer Bot");
        match &st.mode {
            Mode::Sniffing => {
                ui.label("Mode: Sniffing");
            }
            Mode::PassiveToken(mint) => {
                ui.label(format!("Mode: PassiveToken ({mint})"));
            }
            Mode::QuantumManual => {
                ui.label("Mode: Quantum (Manual)");
                
                // Show quantum suggestions
                if !st.quantum_suggestions.is_empty() {
                    ui.separator();
                    ui.heading("🎯 Quantum Suggestions");
                    
                    for suggestion in &st.quantum_suggestions {
                        ui.group(|ui| {
                            ui.horizontal(|ui| {
                                ui.vertical(|ui| {
                                    ui.label(format!("🪙 {}", suggestion.mint));
                                    ui.label(format!("Score: {}%", suggestion.score));
                                    ui.label(format!("Reason: {}", suggestion.reason));
                                });
                                
                                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                                    if ui.button("🛒 BUY").clicked() {
                                        let _ = self.gui_tx.try_send(GuiEvent::Buy(suggestion.mint));
                                    }
                                });
                            });
                            
                            // Show top feature scores
                            ui.collapsing("Feature Scores", |ui| {
                                for (feature, score) in &suggestion.feature_scores {
                                    ui.label(format!("{}: {:.2}", feature, score));
                                }
                            });
                        });
                    }
                } else {
                    ui.label("🔍 Scanning for opportunities...");
                    ui.label("Suggestions will appear when tokens score ≥ 75%");
                }
            }
        }
        
        if let Some(tok) = st.active_token.as_ref() {
            ui.separator();
            ui.label(format!("Active mint: {}", tok.mint));
            if let Some(price) = st.last_buy_price {
                ui.label(format!("Last buy price (mock): {:.4}", price));
            }
            ui.label(format!("Holdings: {:.0}%", st.holdings_percent * 100.0));
        } else if !matches!(st.mode, Mode::QuantumManual) {
            ui.label("No active token");
        }

        // Show sell controls only if we have holdings
        if st.holdings_percent > 0.0 {
            ui.separator();
            ui.horizontal(|ui| {
                if ui.button("Sell 25% (W)").clicked() {
                    let _ = self.gui_tx.try_send(GuiEvent::SellPercent(0.25));
                }
                if ui.button("Sell 50% (Q)").clicked() {
                    let _ = self.gui_tx.try_send(GuiEvent::SellPercent(0.50));
                }
                if ui.button("Sell 100% (S)").clicked() {
                    let _ = self.gui_tx.try_send(GuiEvent::SellPercent(1.0));
                }
            });
            ui.label("Shortcuts: W=25%, Q=50%, S=100%");
        }
    }
}

impl App for BotApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
        ctx.input(|i| {
            if i.key_pressed(Key::W) {
                let _ = self.gui_tx.try_send(GuiEvent::SellPercent(0.25));
            }
            if i.key_pressed(Key::Q) {
                let _ = self.gui_tx.try_send(GuiEvent::SellPercent(0.50));
            }
            if i.key_pressed(Key::S) {
                let _ = self.gui_tx.try_send(GuiEvent::SellPercent(1.0));
            }
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            let st = self.app_state.blocking_lock().clone();
            self.draw_state(ui, &st);
        });

        ctx.request_repaint_after(self.refresh);
    }

    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {
        info!("GUI closed");
    }
}