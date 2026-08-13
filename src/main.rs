// Suppress the console window Windows otherwise opens alongside a GUI app,
// but only in release builds, so eprintln! diagnostics stay visible in dev.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod app;
mod config;
mod decode;
mod dirscan;
mod render;

use std::path::PathBuf;
use winit::event_loop::EventLoop;

fn main() {
    let Some(arg) = std::env::args().nth(1) else {
        eprintln!("usage: warpview <image-file>");
        std::process::exit(1);
    };
    let path = PathBuf::from(arg);
    if !path.is_file() {
        eprintln!("not a file: {}", path.display());
        std::process::exit(1);
    }

    let event_loop = EventLoop::<app::UserEvent>::with_user_event()
        .build()
        .expect("failed to create event loop");
    let proxy = event_loop.create_proxy();
    let mut app = app::App::new(path, proxy);
    event_loop.run_app(&mut app).expect("event loop error");
}
