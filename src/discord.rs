use regex::Regex;
use reqwest::Client;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct DiscordConfig {
  pub webhook_id: String,
  pub webhook_token: String,
}

fn strip_ansi(text: &str) -> String {
  // ANSI escape sequences start with \x1b[ and end with m
  let re = Regex::new(r"\x1b\[[0-9;]*m").unwrap();
  re.replace_all(text, "").to_string()
}

#[allow(dead_code)]
fn filter_diff_content(diff: &str) -> String {
  let clean_diff = strip_ansi(diff);
  let re = Regex::new(r"^[+-][^-+].+").unwrap();
  clean_diff
    .lines()
    .filter(|line| re.is_match(line))
    .take(15) // Limit to 15 lines to avoid Discord embed limits
    .collect::<Vec<&str>>()
    .join("\n")
}

fn json_escape(text: &str) -> String {
  text
    .replace("\\", "\\\\")
    .replace("\"", "\\\"")
    .replace("\n", "\\n")
    .replace("\r", "\\r")
    .replace("\t", "\\t")
}

pub async fn send_discord_notification(
  discord_config: &DiscordConfig,
  title: &str,
  description: &str,
  color: u32,
  diff_content: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  let webhook_url = format!(
    "https://discord.com/api/webhooks/{}/{}",
    discord_config.webhook_id, discord_config.webhook_token
  );

  let final_description = if let Some(diff) = diff_content {
    // Filter diff content to show only additions and deletions
    // let filtered_diff = filter_diff_content(diff);
    // if filtered_diff.is_empty() {
    //   description.to_string()
    // } else {
    //   format!("{}\n\n```diff\n{}```", description, filtered_diff)
    // }
    format!("{}\n\n```diff\n{}```", description, strip_ansi(diff))
  } else {
    description.to_string()
  };

  let json_payload = format!(
    r#"{{
            "username": "Reduced",
            "embeds": [{{
                "title": "{}",
                "description": "{}",
                "color": {},
                "footer": {{
                    "text": "Reduced Notification System"
                }},
                "timestamp": "{}"
            }}]
        }}"#,
    json_escape(title),
    json_escape(&final_description),
    color,
    chrono::Utc::now().to_rfc3339()
  );

  let client = Client::new();
  let response = client
    .post(&webhook_url)
    .header("Content-Type", "application/json")
    .body(json_payload)
    .send()
    .await?;

  if response.status().is_success() {
    Ok(())
  } else {
    Err(format!("Discord webhook failed with status: {}", response.status()).into())
  }
}
