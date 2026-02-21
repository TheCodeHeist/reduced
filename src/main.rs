mod discord;
mod git;
mod mail;

use chrono::Utc;
use clap::{Parser, Subcommand};
use serde::Deserialize;
use std::fs;
use std::sync::Arc;
use std::time::SystemTime;
use tokio::io::AsyncReadExt;
use tokio::net::UnixListener;
use tokio::sync::Mutex;

#[derive(Parser)]
#[command(name = "reduced-daemon")]
#[command(about = "Reduced daemon for handling Oxidized diff and failure alerts")]
struct Cli {
  #[command(subcommand)]
  command: Commands,
}

#[derive(Subcommand)]
enum Commands {
  /// Start the daemon
  Start,
  /// Stop the daemon
  Stop,
  /// Check daemon status
  Status,
}
#[derive(Deserialize, Clone)]
struct Config {
  diff: DiffConfig,
  failure: FailureConfig,
  log: LogConfig,
  report: ReportConfig,
}

#[derive(Deserialize, Clone)]
struct DiffConfig {
  notify: Vec<String>,
  mail: Option<mail::MailConfig>,
  discord: Option<discord::DiscordConfig>,
}

#[derive(Deserialize, Clone)]
struct FailureConfig {
  notify: Vec<String>,
  mail: Option<mail::MailConfig>,
  discord: Option<discord::DiscordConfig>,
  timeout: u64,
}

#[derive(Deserialize, Clone)]
struct LogConfig {
  path: String,
}

#[derive(Deserialize, Clone)]
struct ReportConfig {
  notify: Vec<String>,
  mail: Option<mail::MailConfig>,
  discord: Option<discord::DiscordConfig>,
  interval: u64,
}

#[derive(Clone)]
struct Counts {
  diff: u64,
  failure: u64,
}

const SOCKET_PATH: &str = "/tmp/reduced.sock";
const PID_PATH: &str = "/tmp/reduced.pid";

fn config_path() -> &'static str {
  match std::env::var("HOME") {
    Ok(home) => Box::leak(format!("{}/.config/oxidized/reduced.json", home).into_boxed_str()),
    Err(_) => "~/.config/oxidized/reduced.json",
  }
}

fn expand_tilde(path: &str) -> String {
  if path.starts_with('~') {
    if let Ok(home) = std::env::var("HOME") {
      return path.replacen('~', &home, 1);
    }
  }
  path.to_string()
}

async fn log_to_file(
  log_path: &str,
  message: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  use tokio::fs::OpenOptions;
  use tokio::io::AsyncWriteExt;

  let expanded_path = expand_tilde(log_path);
  let mut file = OpenOptions::new()
    .create(true)
    .append(true)
    .open(&expanded_path)
    .await?;

  let timestamp = Utc::now().format("%Y-%m-%d %H:%M:%S UTC");
  let log_entry = format!("[{}] {}\n", timestamp, message);

  file.write_all(log_entry.as_bytes()).await?;
  file.flush().await?;
  Ok(())
}

async fn validate_and_notify(
  notify_methods: &[String],
  mail_config: Option<&mail::MailConfig>,
  discord_config: Option<&discord::DiscordConfig>,
  subject: &str,
  body: &str,
  title: &str,
  description: &str,
  color: u32,
  diff_content: Option<&str>,
  log_path: &str,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  for method in notify_methods {
    match method.as_str() {
      "mail" => {
        if let Some(config) = mail_config {
          if let Err(e) = mail::send_mail_notification(config, subject, body, diff_content).await {
            log_to_file(
              log_path,
              &format!("Failed to send mail notification: {}", e),
            )
            .await?;
          } else {
            log_to_file(log_path, "Mail notification sent successfully").await?;
          }
        } else {
          log_to_file(log_path, "Method 'mail' is not configured").await?;
        }
      }
      "discord" => {
        if let Some(config) = discord_config {
          if let Err(e) =
            discord::send_discord_notification(config, title, description, color, diff_content)
              .await
          {
            log_to_file(
              log_path,
              &format!("Failed to send Discord notification: {}", e),
            )
            .await?;
          } else {
            log_to_file(log_path, "Discord notification sent successfully").await?;
          }
        } else {
          log_to_file(log_path, "Method 'discord' is not configured").await?;
        }
      }
      _ => {
        log_to_file(
          log_path,
          &format!("Unknown notification method: {}", method),
        )
        .await?;
      }
    }
  }
  Ok(())
}

fn load_config() -> Result<Config, Box<dyn std::error::Error + Send + Sync>> {
  let path = config_path();
  let content = fs::read_to_string(&path)?;
  let mut config: Config = serde_json::from_str(&content)?;
  config.log.path = expand_tilde(&config.log.path);
  Ok(config)
}

fn is_daemon_running() -> bool {
  if let Ok(pid_str) = fs::read_to_string(PID_PATH) {
    if let Ok(pid) = pid_str.trim().parse::<u32>() {
      // Check if process exists
      unsafe { libc::kill(pid as i32, 0) == 0 }
    } else {
      false
    }
  } else {
    false
  }
}

fn start_daemon() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  if is_daemon_running() {
    eprintln!("Daemon is already running");
    return Ok(());
  }

  let daemon = daemonize::Daemonize::new()
    .pid_file(PID_PATH)
    .chown_pid_file(true)
    .working_directory("/tmp")
    .umask(0o027);

  match daemon.start() {
    Ok(_) => {
      println!("Starting daemon...");
      // Daemon started, now run the listener
      let rt = tokio::runtime::Runtime::new().unwrap();
      rt.block_on(run_daemon()).unwrap();
      Ok(())
    }
    Err(e) => Err(Box::new(e)),
  }
  // println!("Starting daemon...");
  // // Daemon started, now run the listener
  // let rt = tokio::runtime::Runtime::new().unwrap();
  // rt.block_on(run_daemon()).unwrap();
  // Ok(())
}

fn stop_daemon() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  if !is_daemon_running() {
    eprintln!("Daemon is not running");
    return Ok(());
  }

  if let Ok(pid_str) = fs::read_to_string(PID_PATH) {
    if let Ok(pid) = pid_str.trim().parse::<u32>() {
      unsafe {
        libc::kill(pid as i32, libc::SIGTERM);
      }
      fs::remove_file(PID_PATH)?;
      println!("Daemon stopped");
    }
  }
  Ok(())
}

fn daemon_status() {
  if is_daemon_running() {
    println!("Daemon is running");
  } else {
    println!("Daemon is not running");
  }
}

async fn run_daemon() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  // Remove existing socket
  let _ = tokio::fs::remove_file(SOCKET_PATH).await;

  let listener = UnixListener::bind(SOCKET_PATH)?;

  let config = load_config()?;
  let last_failure_time = Arc::new(Mutex::new(None::<SystemTime>));
  let counts = Arc::new(Mutex::new(Counts {
    diff: 0,
    failure: 0,
  }));

  // Spawn report task
  {
    let counts = Arc::clone(&counts);
    let config = config.clone();
    tokio::spawn(async move {
      let mut interval = tokio::time::interval(std::time::Duration::from_secs(
        config.report.interval * 86400,
      ));
      loop {
        interval.tick().await;
        let mut counts_guard = counts.lock().await;
        if counts_guard.diff > 0 || counts_guard.failure > 0 {
          if let Err(e) = log_to_file(
            &config.log.path,
            &format!(
              "Report sent: {} diffs, {} failures",
              counts_guard.diff, counts_guard.failure
            ),
          )
          .await
          {
            eprintln!("Error logging report: {}", e);
          }

          let subject = format!(
            "Oxidized Report: {} changes, {} failures",
            counts_guard.diff, counts_guard.failure
          );
          let body = format!(
            "<html><body><h2>📊 Oxidized Activity Report (by Reduced)</h2><p><strong>Report Time:</strong> {}</p><p><strong>Configuration Changes:</strong> {}</p><p><strong>Backup Failures:</strong> {}</p><hr><p>This is an automated report from Reduced.</p></body></html>",
            Utc::now().to_rfc3339(),
            counts_guard.diff,
            counts_guard.failure
          );
          let title = "📊 Oxidized Activity Report";
          let description = format!(
            "**Configuration Changes:** {}\n**Backup Failures:** {}\n**Report Time:** {}",
            counts_guard.diff,
            counts_guard.failure,
            Utc::now().to_rfc3339()
          );

          if let Err(e) = validate_and_notify(
            &config.report.notify,
            config.report.mail.as_ref(),
            config.report.discord.as_ref(),
            &subject,
            &body,
            title,
            &description,
            3447003, // Blue
            None,
            &config.log.path,
          )
          .await
          {
            eprintln!("Error sending report notifications: {}", e);
          }

          // Reset counts
          counts_guard.diff = 0;
          counts_guard.failure = 0;
        }
      }
    });
  }

  println!("Daemon listening on {}", SOCKET_PATH);
  loop {
    let (mut socket, _) = listener.accept().await?;
    let config = config.clone();
    let last_failure_time = Arc::clone(&last_failure_time);
    let counts = Arc::clone(&counts);

    tokio::spawn(async move {
      let mut buf = [0; 4096]; // Increase buffer size
      let n = socket.read(&mut buf).await.unwrap_or(0);
      let message = String::from_utf8_lossy(&buf[..n]).trim().to_string();

      let mut lines = message.lines();
      let request = lines.next().unwrap_or("").to_string();
      let env_vars: Vec<(String, String)> = lines
        .filter_map(|line| {
          let mut parts = line.splitn(2, '=');
          Some((parts.next()?.to_string(), parts.next()?.to_string()))
        })
        .collect();

      match request.as_str() {
        "diff" => {
          println!("Processing diff request");
          let node_name = env_vars
            .iter()
            .find(|(k, _)| k == "OX_NODE_NAME")
            .map(|(_, v)| v.as_str())
            .unwrap_or("unknown");
          {
            let mut counts = counts.lock().await;
            counts.diff += 1;
          }
          if let Err(e) = log_to_file(
            &config.log.path,
            &format!("Diff request processed for node: {}", node_name),
          )
          .await
          {
            eprintln!("Error logging diff request: {}", e);
          }

          // Get diff content from git repository if available
          let diff_content = if let Some(git_config) = git::extract_git_config_from_env(&env_vars) {
            match git::get_git_diff_content(&git_config).await {
              Ok(content) => Some(content),
              Err(e) => {
                eprintln!("Error fetching git diff: {}", e);
                Some(format!("Error fetching diff for {}: {}", node_name, e))
              }
            }
          } else {
            None
          };

          let subject = format!("Config Change: {}", node_name);
          let body = format!(
            "<html><body><h2>Configuration Change Detected</h2><p>Device: {}</p><p>Time: {}</p><p>Configuration has been updated and committed to git.</p></body></html>",
            node_name,
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
          );
          let title = "🔄 Config Change Detected";
          let description = format!(
            "**Device:** {}\n**Time:** {}\nConfiguration has been updated and committed to git.",
            node_name,
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC")
          );

          if let Err(e) = validate_and_notify(
            &config.diff.notify,
            config.diff.mail.as_ref(),
            config.diff.discord.as_ref(),
            &subject,
            &body,
            title,
            &description,
            3066993, // Green
            diff_content.as_deref(),
            &config.log.path,
          )
          .await
          {
            eprintln!("Error sending diff notifications: {}", e);
          }
        }
        "failure" => {
          println!("Processing failure request");
          let node_name = env_vars
            .iter()
            .find(|(k, _)| k == "OX_NODE_NAME")
            .map(|(_, v)| v.as_str())
            .unwrap_or("unknown");
          let mut last_time = last_failure_time.lock().await;
          let now = SystemTime::now();
          if let Some(last) = *last_time {
            if now.duration_since(last).unwrap_or_default().as_secs() < config.failure.timeout {
              println!("Failure request ignored due to timeout");
              if let Err(e) = log_to_file(
                &config.log.path,
                &format!("Failure request ignored (timeout) for node: {}", node_name),
              )
              .await
              {
                eprintln!("Error logging ignored failure request: {}", e);
              }
              return;
            }
          }
          *last_time = Some(now);
          drop(last_time);

          {
            let mut counts = counts.lock().await;
            counts.failure += 1;
          }
          if let Err(e) = log_to_file(
            &config.log.path,
            &format!("Failure request processed for node: {}", node_name),
          )
          .await
          {
            eprintln!("Error logging failure request: {}", e);
          }

          let ip = env_vars
            .iter()
            .find(|(k, _)| k == "OX_NODE_IP")
            .map(|(_, v)| v.as_str())
            .unwrap_or("unknown");
          let group = env_vars
            .iter()
            .find(|(k, _)| k == "OX_NODE_GROUP")
            .map(|(_, v)| v.as_str())
            .unwrap_or("none");
          let msg = env_vars
            .iter()
            .find(|(k, _)| k == "OX_NODE_MSG")
            .map(|(_, v)| v.as_str())
            .unwrap_or("unknown error");

          let subject = format!("Oxidized FAILURE: {}", node_name);
          let body = format!(
            "<html><body><h2 style='color:red'>⚠️ Oxidized Backup FAILED</h2><p><strong>Device:</strong> {}</p><p><strong>IP:</strong> {}</p><p><strong>Group:</strong> {}</p><p><strong>Time:</strong> {}</p><p><strong>Reason:</strong> {}</p><hr><p>Please investigate connectivity or credentials.</p></body></html>",
            node_name,
            ip,
            group,
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            msg
          );
          let title = "⚠️ Oxidized Backup FAILED";
          let description = format!(
            "**Device:** {}\n**IP:** {}\n**Group:** {}\n**Time:** {}\n**Reason:** {}",
            node_name,
            ip,
            group,
            Utc::now().format("%Y-%m-%d %H:%M:%S UTC"),
            msg
          );

          if let Err(e) = validate_and_notify(
            &config.failure.notify,
            config.failure.mail.as_ref(),
            config.failure.discord.as_ref(),
            &subject,
            &body,
            title,
            &description,
            15158332, // Red
            None,
            &config.log.path,
          )
          .await
          {
            eprintln!("Error sending failure notifications: {}", e);
          }
        }
        _ => {
          eprintln!("Unknown request: {}", request);
          if let Err(e) =
            log_to_file(&config.log.path, &format!("Unknown request: {}", request)).await
          {
            eprintln!("Error logging unknown request: {}", e);
          }
        }
      }
    });
  }
}

#[allow(dead_code)]
async fn run_script(
  script_path: &str,
  env_vars: &[(String, String)],
) -> Result<(), Box<dyn std::error::Error>> {
  let expanded_path = expand_tilde(script_path);
  let mut cmd = tokio::process::Command::new("sh");
  cmd.arg(&expanded_path);
  for (key, value) in env_vars {
    cmd.env(key, value);
  }
  let output = cmd.output().await?;

  if output.status.success() {
    println!("Script executed successfully");
  } else {
    eprintln!("Script failed: {}", String::from_utf8_lossy(&output.stderr));
  }

  Ok(())
}

fn main() -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let cli = Cli::parse();

  match cli.command {
    Commands::Start => start_daemon()?,
    Commands::Stop => stop_daemon()?,
    Commands::Status => daemon_status(),
  }

  Ok(())
}
