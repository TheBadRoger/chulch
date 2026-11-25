mod cli;
use std::process::ExitCode;

fn main() -> ExitCode {
    match cli::run() {
        Ok(0) => ExitCode::SUCCESS,
        Ok(code) => std::process::exit(code),
        Err(err) => {
            eprintln!("mcdocker: {err:#}");
            ExitCode::FAILURE
        }
    }
}
