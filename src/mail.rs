use std::process::Stdio;
use tokio::io::AsyncWriteExt;
use tokio::process::Command;

#[derive(Debug, Clone, serde::Deserialize)]
pub struct MailConfig {
  pub recipients: Vec<String>,
  pub cc: Option<Vec<String>>,
  pub bcc: Option<Vec<String>>,
}

fn ansi_to_html(ansi_text: &str) -> String {
  let mut html = String::new();
  let mut chars = ansi_text.chars().peekable();
  let mut current_color = String::new();

  while let Some(ch) = chars.next() {
    if ch == '\x1b' && chars.peek() == Some(&'[') {
      // Skip the '['
      chars.next();

      // Parse ANSI color code
      let mut code = String::new();
      while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() || c == ';' {
          code.push(chars.next().unwrap());
        } else if c == 'm' {
          chars.next(); // consume 'm'
          break;
        } else {
          break;
        }
      }

      // Close previous color span
      if !current_color.is_empty() {
        html.push_str("</span>");
      }

      // Start new color span based on ANSI code
      match code.as_str() {
        "31" => {
          // Red (deletions)
          html.push_str("<span style=\"color: #dc3545;\">");
          current_color = "red".to_string();
        }
        "32" => {
          // Green (additions)
          html.push_str("<span style=\"color: #28a745;\">");
          current_color = "green".to_string();
        }
        "33" => {
          // Yellow (context/function names)
          html.push_str("<span style=\"color: #ffc107;\">");
          current_color = "yellow".to_string();
        }
        "34" => {
          // Blue
          html.push_str("<span style=\"color: #007bff;\">");
          current_color = "blue".to_string();
        }
        "35" => {
          // Magenta
          html.push_str("<span style=\"color: #6f42c1;\">");
          current_color = "magenta".to_string();
        }
        "36" => {
          // Cyan
          html.push_str("<span style=\"color: #17a2b8;\">");
          current_color = "cyan".to_string();
        }
        "0" | "" => {
          // Reset
          current_color.clear();
        }
        _ => {
          // Unknown code, ignore
        }
      }
    } else {
      // Regular character - escape HTML entities
      match ch {
        '&' => html.push_str("&amp;"),
        '<' => html.push_str("&lt;"),
        '>' => html.push_str("&gt;"),
        '"' => html.push_str("&quot;"),
        '\'' => html.push_str("&#x27;"),
        '\n' => html.push_str("<br>"),
        _ => html.push(ch),
      }
    }
  }

  // Close any remaining color span
  if !current_color.is_empty() {
    html.push_str("</span>");
  }

  html
}

pub async fn send_mail_notification(
  mail_config: &MailConfig,
  subject: &str,
  body: &str,
  diff_content: Option<&str>,
) -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
  // Build the email body with diff content if available
  let mut email_body = body.to_string();

  if let Some(diff) = diff_content {
    let html_diff = ansi_to_html(diff);
    email_body.push_str(&format!(
      "<br><br><h3>Configuration Changes:</h3><pre style=\"background-color: #f5f5f5; padding: 10px; border: 1px solid #ddd; font-family: 'Courier New', monospace; white-space: pre-wrap; line-height: 1.2;\">{}</pre>",
      html_diff
    ));
  }

  // For now, we'll use the mail command. In a production system, you'd want to use a proper SMTP library
  let recipients = mail_config.recipients.join(",");

  let mut cmd = Command::new("mail");
  cmd.arg("-s").arg(subject);
  cmd.arg("-a").arg("Content-Type: text/html");

  if let Some(cc_list) = &mail_config.cc {
    if !cc_list.is_empty() {
      cmd.arg("-c").arg(cc_list.join(","));
    }
  }

  if let Some(bcc_list) = &mail_config.bcc {
    if !bcc_list.is_empty() {
      cmd.arg("-b").arg(bcc_list.join(","));
    }
  }

  cmd.arg(&recipients);

  let mut child = cmd.stdin(Stdio::piped()).spawn()?;
  if let Some(stdin) = &mut child.stdin {
    stdin.write_all(email_body.as_bytes()).await?;
  }

  let status = child.wait().await?;
  if status.success() {
    Ok(())
  } else {
    Err(format!("Mail command failed with status: {}", status).into())
  }
}
