#![cfg_attr(all(target_os = "windows", not(test)), windows_subsystem = "windows")]

mod app;
mod classifier;
mod config;
mod input;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let loaded_config = config::AppConfig::load_from_working_dir_with_diagnostics();
    let config = loaded_config.config;
    show_debug_console(config.debug);
    init_logging(config.debug);

    if let Some(warning) = loaded_config.warning {
        log::warn!("{warning}");
    }

    app::run(config)?;

    Ok(())
}

fn init_logging(debug: bool) {
    let default_filter = if debug { "debug" } else { "info" };
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or(default_filter))
        .init();
}

#[cfg(target_os = "windows")]
fn show_debug_console(debug: bool) {
    if !debug {
        return;
    }

    unsafe {
        use windows_sys::Win32::System::Console::{
            ATTACH_PARENT_PROCESS, AllocConsole, AttachConsole,
        };

        if AttachConsole(ATTACH_PARENT_PROCESS) == 0 {
            let _ = AllocConsole();
        }
    }
}

#[cfg(not(target_os = "windows"))]
fn show_debug_console(_debug: bool) {}
