// Release builds run without a console window; debug builds keep stderr visible.
#![cfg_attr(all(windows, not(debug_assertions)), windows_subsystem = "windows")]

mod platform;
mod ui;

fn main() {
    if let Err(error) = ui::run() {
        eprintln!("EcoPasteNeo: {error:#}");
        std::process::exit(1);
    }
}
