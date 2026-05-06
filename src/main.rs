mod app;
mod capture;
mod cli;
mod config;
mod error;
mod input;
mod output;
mod overlay;
mod render;
mod shm;
mod state;
mod wayland;
mod window;
mod zoom;

fn main() {
    app::init_tracing();

    if let Err(err) = app::run() {
        eprintln!("error: {err}");
        std::process::exit(1);
    }
}
