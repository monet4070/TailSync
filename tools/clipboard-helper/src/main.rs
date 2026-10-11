//! Short-lived Windows I/O process. The daemon imposes a killable deadline.
mod analysis;
#[cfg(windows)]
mod windows;

fn analysis_command(args: &[String]) -> Option<i32> {
    if args.len() != 1 {
        return None;
    }
    let command = analysis::command(&args[0])?;
    let outcome =
        analysis::read_bounded_stdin().and_then(|bytes| analysis::analyze(command, &bytes));
    match outcome {
        Ok(result) => {
            let frame = analysis::encode_or_exit(&result);
            match std::io::Write::write_all(&mut std::io::stdout(), &frame) {
                Ok(()) => Some(0),
                Err(_) => Some(65),
            }
        }
        Err(_) => Some(2),
    }
}

#[cfg(windows)]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = analysis_command(&args) {
        std::process::exit(code);
    }
    if windows::run().is_err() {
        std::process::exit(65);
    }
}
#[cfg(not(windows))]
fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = analysis_command(&args) {
        std::process::exit(code);
    }
    eprintln!("Use the AppKit helper on macOS");
    std::process::exit(64);
}
