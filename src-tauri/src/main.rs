//! Tauri application entry point.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    dna_assistant_lib::run();
}
