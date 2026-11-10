use std::process::ExitCode;

use env_logger::Env;

fn main() -> ExitCode {
    // `pdf-extract` warns once per page for standard Type 1 glyph names such as
    // `.notdef`, which carry no text and only add noise. Extraction failures are
    // reported by the input layer, and `RUST_LOG` still overrides this default.
    env_logger::Builder::from_env(Env::default().default_filter_or("warn,pdf_extract=error"))
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
