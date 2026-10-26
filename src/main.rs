#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod capture;
mod desktop_icons;
mod fun;
mod log;
mod panel;
mod region;
mod render;
mod settings;
mod snapshot;
mod stats;
mod sysmon;
mod texture;
mod ui;
mod win;

fn main() {
    let args: Vec<String> = std::env::args().collect();
    log::init();
    std::panic::set_hook(Box::new(|info| {
        log::error(&format!("panic: {info}"));
    }));

    win::set_process_dpi_aware();
    if args.iter().any(|a| a == "--snapshot") {
        snapshot::run(&args);
        return;
    }
    if args.iter().any(|a| a == "--icons") {
        desktop_icons::debug_dump();
        return;
    }

    if let Err(error) = app::run() {
        log::error(&format!("fatal: {error}"));
        win::fatal_message(&error);
    }
}
