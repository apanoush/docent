use std::process::ExitCode;

use env_logger::Env;

fn main() -> ExitCode {
    // The PDF backends warn about font details that Docent already handles or
    // reports as extraction errors. Keep those log targets quiet so a broken
    // encoding does not flood stderr; `RUST_LOG` still overrides this default.
    env_logger::Builder::from_env(
        Env::default().default_filter_or("warn,pdf_extract=error,pdf_inspector=error,lopdf=error"),
    )
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
