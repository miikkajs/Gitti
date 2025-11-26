mod app;
mod git;
mod highlighter;
mod theme;
mod types;
mod ui;

use clap::Parser;
use crossterm::{
    cursor::Show,
    execute,
    terminal::{self, LeaveAlternateScreen},
};
use std::io;
use std::path::PathBuf;

use app::App;

#[derive(Parser)]
#[command(name = "gitti")]
#[command(about = "Fast git diff viewer with IntelliJ-style output", long_about = None)]
struct Cli {
    /// Show staged changes
    #[arg(long, short)]
    staged: bool,

    /// Compare with specific commit
    #[arg(long, short)]
    commit: Option<String>,

    /// Specific file to diff
    #[allow(dead_code)]
    file: Option<PathBuf>,

    /// Context lines around changes (default 5)
    #[arg(long, short = 'C', default_value = "5")]
    context: usize,
}

fn main() {
    let cli = Cli::parse();

    // Set up panic hook to show backtrace
    std::panic::set_hook(Box::new(|panic_info| {
        // Restore terminal first
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);

        // Print panic info
        eprintln!("\n\x1b[31m=== Application Crashed ===\x1b[0m\n");

        if let Some(location) = panic_info.location() {
            eprintln!(
                "Location: {}:{}:{}",
                location.file(),
                location.line(),
                location.column()
            );
        }

        if let Some(message) = panic_info.payload().downcast_ref::<&str>() {
            eprintln!("Message: {}", message);
        } else if let Some(message) = panic_info.payload().downcast_ref::<String>() {
            eprintln!("Message: {}", message);
        }

        eprintln!("\nBacktrace:");
        eprintln!("{}", std::backtrace::Backtrace::force_capture());
    }));

    // Create and run app
    let mut app = match App::new(cli.staged, cli.commit, cli.context) {
        Ok(app) => app,
        Err(e) => {
            eprintln!("Error: {}", e);
            std::process::exit(1);
        }
    };

    if !app.has_files() {
        println!("No changes detected.");
        return;
    }

    if let Err(e) = app.run() {
        let _ = terminal::disable_raw_mode();
        let _ = execute!(io::stdout(), Show, LeaveAlternateScreen);
        eprintln!("Error: {}", e);
        std::process::exit(1);
    }
}
