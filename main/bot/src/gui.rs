use std::collections::VecDeque;
use std::fs;
use std::sync::Arc;
use std::time::Duration;
use anyhow::Result;
use eframe::egui::{self, Key, Color32, RichText, ScrollArea, Stroke};
use eframe::{App, Frame};
use solana_sdk::pubkey::Pubkey;
use tokio::sync::{mpsc::Sender, Mutex};
use tracing::info;
use crate::types::{AppState, Mode, QuantumCandidateGui};

// --- Zdarzenia i Typy ---

#[derive(Clone, Debug)]
pub enum GuiEvent {
SellPercent(f64),
Buy(Pubkey),
ReloadStyle, // Nowe zdarzenie do przeładowania stylu
}
pub type GuiEventSender = Sender<GuiEvent>;

// Nowa, lekka struktura do przekazywania zdarzeń do logu w GUI
#[derive(Clone, Debug)]
pub struct GuiLogEvent {
pub timestamp: String,
pub message: String,
pub level: String, // "INFO", "WARN", "ERROR"
}

#[derive(Clone, Debug)]
pub struct GuiState {
pub mode: Mode,
pub active_token_mint: Option<String>,
pub last_buy_price: Option<f64>,
pub holdings_percent: f64,
pub quantum_suggestions: Vec<QuantumCandidateGui>,
// Przechowuje ostatnie zdarzenia
pub log_events: VecDeque<GuiLogEvent>,
// Aktywny styl interfejsu
pub active_style: egui::Style,
}

impl GuiState {
    /// Convert from AppState to GuiState
    pub fn from_app_state(app_state: &AppState) -> Self {
        let active_token_mint = app_state.active_token.as_ref()
            .map(|token| token.mint.to_string());
        
        Self {
            mode: app_state.mode.clone(),
            active_token_mint,
            last_buy_price: app_state.last_buy_price,
            holdings_percent: app_state.holdings_percent,
            quantum_suggestions: app_state.quantum_suggestions.clone(),
            log_events: VecDeque::with_capacity(10), // Start with empty log events
            active_style: egui::Style::default(),
        }
    }
}

impl Default for GuiState {
fn default() -> Self {
Self {
mode: Mode::Sniffing,
active_token_mint: None,
last_buy_price: None,
holdings_percent: 0.0,
quantum_suggestions: Vec::new(),
log_events: VecDeque::with_capacity(10), // Przechowuj np. 10 ostatnich logów
active_style: egui::Style::default(),
}
}
}

// --- Uruchomienie GUI ---

pub fn launch_gui(
title: &str,
app_state: Arc<Mutex<AppState>>,
gui_tx: GuiEventSender,
refresh: Duration,
) -> Result<()> {
let native_options = eframe::NativeOptions::default();
let app = BotApp::new(app_state, gui_tx, refresh);
eframe::run_native(title, native_options, Box::new(|_| Box::new(app)))
.map_err(|e| anyhow::anyhow!("GUI error: {}", e))
}

// --- Aplikacja GUI ---

struct BotApp {
app_state_handle: Arc<Mutex<AppState>>,
local_gui_state: GuiState,
gui_tx: GuiEventSender,
refresh: Duration,
}

impl BotApp {
fn new(app_state_handle: Arc<Mutex<AppState>>, gui_tx: GuiEventSender, refresh: Duration) -> Self {
Self {
app_state_handle,
local_gui_state: GuiState::default(),
gui_tx,
refresh,
}
}

// --- Logika Rysowania Interfejsu ---  

fn draw_state(&self, ui: &mut egui::Ui, st: &GuiState) {  
    // --- Górny panel: Status i akcje ---  
    ui.vertical_centered(|ui| {  
        ui.heading("SNIPER Bot");  
    });  

    ui.separator();  
      
    // --- Panel Statusu ---  
    egui::Grid::new("status_grid").num_columns(2).show(ui, |ui| {  
        ui.label("Mode:");  
        ui.label(format!("{:?}", st.mode));  
        ui.end_row();  

        if let Some(mint) = &st.active_token_mint {  
             ui.label("Active Token:");  
             ui.label(mint);  
             ui.end_row();  
        }  
    });  
      
    // ULEPSZENIE: Pasek postępu dla posiadanych tokenów  
    if st.holdings_percent > 0.0 {  
        ui.add_space(5.0);  
        let holdings_text = format!("Holdings: {:.1}%", st.holdings_percent * 100.0);  
        ui.add(egui::ProgressBar::new(st.holdings_percent as f32).text(holdings_text));  
        ui.add_space(5.0);  
    }  

    ui.separator();  

    // --- Panel Akcji (Sprzedaż) ---  
    if st.holdings_percent > 0.0 {  
        ui.horizontal(|ui| {  
             ui.label("Actions:");  
             if ui.button(RichText::new("Sell 25% (W)").color(Color32::from_rgb(255, 200, 100))).clicked() {  
                let _ = self.gui_tx.try_send(GuiEvent::SellPercent(0.25));  
            }  
            if ui.button(RichText::new("Sell 50% (Q)").color(Color32::from_rgb(255, 150, 80))).clicked() {  
                let _ = self.gui_tx.try_send(GuiEvent::SellPercent(0.50));  
            }  
            if ui.button(RichText::new("Sell 100% (S)").color(Color32::from_rgb(255, 80, 80))).clicked() {  
                let _ = self.gui_tx.try_send(GuiEvent::SellPercent(1.0));  
            }  
        });  
        ui.separator();  
    }  

    // --- Panel Sugestii Quantum ---  
    ui.heading("🎯 Quantum Suggestions");  
    ScrollArea::vertical().show(ui, |ui| {  
        if !st.quantum_suggestions.is_empty() {  
            // ULEPSZENIE: Dynamiczne sortowanie sugestii  
            let mut suggestions = st.quantum_suggestions.clone();  
            suggestions.sort_by(|a, b| b.score.cmp(&a.score));  

            for suggestion in suggestions {  
                // ULEPSZENIE: Kolorowe sygnały wizualne  
                let score_color = get_color_for_score(suggestion.score);  
                let frame = egui::Frame::group(ui.style()).stroke(Stroke::new(1.0, score_color));  

                frame.show(ui, |ui| {  
                    ui.horizontal(|ui| {  
                        ui.vertical(|ui| {  
                            ui.label(RichText::new(format!("🪙 {}", suggestion.mint)).strong());  
                            ui.label(RichText::new(format!("Score: {}%", suggestion.score)).color(score_color).strong());  
                            ui.label(format!("Reason: {}", suggestion.reason));  
                        });  

                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {  
                            if ui.button(RichText::new("🛒 BUY").size(16.0)).clicked() {  
                                let _ = self.gui_tx.try_send(GuiEvent::Buy(suggestion.mint));  
                            }  
                        });  
                    });  
                });  
            }  
        } else {  
            ui.label("🔍 Scanning for opportunities...");  
        }  
    });  
}  
  
// --- ULEPSZENIE: Panel Logów ---  
fn draw_log_panel(&self, ui: &mut egui::Ui, st: &GuiState) {  
    ui.separator();  
    ui.heading("📜 Event Log");  
      
    let frame = egui::Frame::group(ui.style()).inner_margin(egui::vec2(5.0, 5.0));  
    frame.show(ui, |ui| {  
        ScrollArea::vertical().stick_to_bottom(true).show(ui, |ui| {  
            for event in &st.log_events {  
                let color = match event.level.as_str() {  
                    "INFO" => Color32::from_gray(200),  
                    "WARN" => Color32::from_rgb(255, 215, 0), // Gold  
                    "ERROR" => Color32::from_rgb(255, 69, 0), // OrangeRed  
                    _ => Color32::WHITE,  
                };  
                ui.label(RichText::new(format!("[{}] {}", event.timestamp, event.message)).color(color));  
            }  
        });  
    });  
}

}

// --- Pętla Aplikacji ---
impl App for BotApp {
fn update(&mut self, ctx: &egui::Context, _frame: &mut Frame) {
// --- Obsługa skrótów klawiszowych ---
ctx.input(|i| {
if i.key_pressed(Key::W) { let _ = self.gui_tx.try_send(GuiEvent::SellPercent(0.25)); }
if i.key_pressed(Key::Q) { let _ = self.gui_tx.try_send(GuiEvent::SellPercent(0.50)); }
if i.key_pressed(Key::S) { let _ = self.gui_tx.try_send(GuiEvent::SellPercent(1.0)); }
});

// --- Nieblokujące pobieranie stanu ---  
    if let Ok(guard) = self.app_state_handle.try_lock() {  
        self.local_gui_state = GuiState::from_app_state(&guard);  
    }  

    // --- ULEPSZENIE: Zastosowanie stylu ---  
    ctx.set_style(self.local_gui_state.active_style.clone());  

    // --- Główny panel ---  
    egui::CentralPanel::default().show(ctx, |ui| {  
        self.draw_state(ui, &self.local_gui_state);  
        self.draw_log_panel(ui, &self.local_gui_state);  
          
        // --- ULEPSZENIE: Przycisk do przeładowania stylu ---  
        ui.add_space(10.0);  
         if ui.button("🎨 Reload Style").clicked() {  
            let _ = self.gui_tx.try_send(GuiEvent::ReloadStyle);  
        }  
    });  

    ctx.request_repaint_after(self.refresh);  
}  

fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {  
    info!("GUI closed");  
}

}

// --- Funkcje pomocnicze ---

// Helper do kolorowania sugestii na podstawie wyniku
fn get_color_for_score(score: u8) -> Color32 {
match score {
85..=100 => Color32::from_rgb(0, 255, 127), // SpringGreen
70..=84 => Color32::from_rgb(173, 255, 47), // GreenYellow
50..=69 => Color32::from_rgb(255, 215, 0), // Gold
_ => Color32::from_rgb(255, 69, 0),      // OrangeRed
}
}

// Helper do wczytywania stylu z pliku
pub fn load_style_from_file(path: &str) -> Result<egui::Style> {
    // Since egui::Style doesn't implement Deserialize, we'll return the default style
    // In a real implementation, you would manually parse style properties from JSON
    let _style_json = fs::read_to_string(path)?;
    info!("Style file loaded from: {}", path);
    Ok(egui::Style::default())
}
