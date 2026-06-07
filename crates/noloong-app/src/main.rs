#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    let options = match noloong_app::AppLaunchOptions::from_env_or_default() {
        Ok(options) => options,
        Err(error) => {
            eprintln!("{error}");
            std::process::exit(1);
        }
    };
    if let Err(error) = noloong_app::run_app(options) {
        eprintln!("{error}");
        std::process::exit(1);
    }
}
