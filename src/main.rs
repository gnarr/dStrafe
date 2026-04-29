mod app;
mod classifier;
mod config;
mod input;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info")).init();

    let config = config::AppConfig::load_from_working_dir();
    app::run(config)?;

    Ok(())
}
