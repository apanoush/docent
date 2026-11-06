use std::process::ExitCode;

use env_logger::Env;

fn main() -> ExitCode {
    env_logger::Builder::from_env(Env::default().default_filter_or("warn"))
        .format_timestamp(None)
        .init();

    match docent::cli::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {error}");
            ExitCode::from(error.exit_code())
        }
    }
}
