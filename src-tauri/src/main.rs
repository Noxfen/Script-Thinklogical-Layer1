// Nessuna finestra console su Windows in release: l'operatore vede solo l'app.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    tlx_control_lib::run();
}
