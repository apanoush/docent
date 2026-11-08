//! Native messaging host for the Firefox adapter.
//!
//! Firefox starts this binary with the manifest path and the add-on ID as
//! arguments. Both are ignored: the request on stdin names the `docent`
//! executable, and the host manifest's `allowed_extensions` decides which
//! add-ons may connect.

use std::process::ExitCode;

fn main() -> ExitCode {
    match docent::native_host::run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("docent-native-host: {error}");
            ExitCode::FAILURE
        }
    }
}
