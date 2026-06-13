mod build_info;
mod build_info_cli;
mod chatgpt;
mod cli;
mod config;
mod host;
mod models_dev;
mod profile_config_cli;
mod runtime_control;
mod schema;
mod telegram_cli;
#[cfg(test)]
mod test_support;
mod weixin_cli;

#[tokio::main]
async fn main() {
    init_process_diagnostics();
    if let Err(error) = cli::run_cli(std::env::args().skip(1).collect()).await {
        eprintln!("{error}");
        std::process::exit(1);
    }
}

fn init_process_diagnostics() {
    human_panic::setup_panic!();
    let _ = env_logger::Builder::from_env(env_logger::Env::default().default_filter_or("info"))
        .try_init();
}
