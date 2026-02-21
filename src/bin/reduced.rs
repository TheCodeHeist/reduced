use clap::{Parser, Subcommand};
use std::env;
use tokio::io::AsyncWriteExt;

#[derive(Parser)]
#[command(name = "reduced")]
#[command(about = "Reduced client for sending requests to the daemon")]
struct Cli {
  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand)]
enum Commands {
  /// Send a diff mail request
  Diff,
  /// Send a failure mail request
  Failure,
}

const SOCKET_PATH: &str = "/tmp/reduced.sock";

async fn send_request(request: &str) -> Result<(), Box<dyn std::error::Error>> {
  match tokio::net::UnixStream::connect(SOCKET_PATH).await {
    Ok(mut stream) => {
      // Collect OX_ environment variables
      let ox_vars: Vec<String> = env::vars()
        .filter(|(key, _)| key.starts_with("OX_"))
        .map(|(key, value)| format!("{}={}", key, value))
        .collect();

      let message = format!("{}\n{}", request, ox_vars.join("\n"));
      stream.write_all(message.as_bytes()).await?;
      stream.shutdown().await?;
      Ok(())
    }
    Err(_) => {
      eprintln!("Warning: Reduced daemon is not running. Mail request ignored.");
      Ok(())
    }
  }
}
#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
  let cli = Cli::parse();

  match cli.command {
    Commands::Diff => send_request("diff").await?,
    Commands::Failure => send_request("failure").await?,
  }

  Ok(())
}
