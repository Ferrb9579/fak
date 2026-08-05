#![forbid(unsafe_code)]

use clap::Parser;
use fak::{app_alias, fix_command, last_command, show_diff};

#[derive(Parser, Debug)]
#[command(name = "fak", about = "Correct previous console commands")]
struct Cli {
    /// Print a shell function that feeds recent history via FAK_HISTORY.
    /// Add to your shell: eval "$(fak --alias)"
    /// During dev:        eval "$(cargo run --quiet -- --alias)"
    #[arg(long)]
    alias: Option<Option<String>>,

    /// Internal mode used by the generated shell wrapper to update parent-shell history.
    #[arg(long, hide = true)]
    shell_command: bool,

    /// Optional forced command (same role as thefuck CLI args).
    #[arg(trailing_var_arg = true, allow_hyphen_values = true)]
    command: Vec<String>,
}

#[tokio::main]
async fn main() {
    dotenvy::dotenv().ok();
    let cli = Cli::parse();

    if let Some(alias_opt) = cli.alias {
        let name = alias_opt.unwrap_or_else(|| "fak".to_string());
        print!("{}", app_alias(&name));
        return;
    }

    let cmd_to_fix = match last_command(&cli.command) {
        Ok(cmd) => cmd,
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    };

    match fix_command(&cmd_to_fix).await {
        Ok(fixed) => {
            if cli.shell_command {
                eprintln!("{}", show_diff(&cmd_to_fix, &fixed));
                eprintln!("Press Enter to run it or Ctrl+C to cancel");

                let mut input = String::new();
                if std::io::stdin().read_line(&mut input).is_err() {
                    std::process::exit(1);
                }

                println!("{fixed}");
                return;
            }

            println!("{}", show_diff(&cmd_to_fix, &fixed));
            println!("Press Enter to run it or Ctrl+C to cancel");

            let mut input = String::new();
            std::io::stdin().read_line(&mut input).ok();

            let status = std::process::Command::new("sh")
                .arg("-c")
                .arg(&fixed)
                .stdin(std::process::Stdio::inherit())
                .stdout(std::process::Stdio::inherit())
                .stderr(std::process::Stdio::inherit())
                .status();

            match status {
                Ok(status) => {
                    if let Some(code) = status.code() {
                        std::process::exit(code);
                    }
                }
                Err(e) => {
                    eprintln!("Failed to execute command: {e}");
                    std::process::exit(1);
                }
            }
        }
        Err(e) => {
            eprintln!("error: {e}");
            std::process::exit(1);
        }
    }
}
