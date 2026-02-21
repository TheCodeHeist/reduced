use std::process::Stdio;
use tokio::process::Command;

#[derive(Debug, Clone)]
pub struct GitConfig {
  pub repo_name: String,
  pub commit_ref: String,
  pub node_name: String,
  pub node_group: Option<String>,
  pub job_status: Option<String>,
  pub job_time: Option<String>,
}

pub async fn get_git_diff_content(
  git_config: &GitConfig,
) -> Result<String, Box<dyn std::error::Error + Send + Sync>> {
  // First verify the commit exists
  let verify_result = Command::new("git")
    .arg("--bare")
    .arg("--git-dir")
    .arg(&git_config.repo_name)
    .arg("rev-parse")
    .arg("--quiet")
    .arg("--verify")
    .arg(&git_config.commit_ref)
    .stdout(Stdio::null())
    .stderr(Stdio::null())
    .status()
    .await?;

  let mut output = format!("Node name: {}\n", git_config.node_name);

  if let Some(group) = &git_config.node_group {
    output.push_str(&format!("Group name: {}\n", group));
  }

  if let Some(status) = &git_config.job_status {
    output.push_str(&format!("Job status: {}\n", status));
  }

  if let Some(time) = &git_config.job_time {
    output.push_str(&format!("Job time: {}\n", time));
  }

  output.push_str(&format!("Git repo: {}\n", git_config.repo_name));
  output.push_str(&format!("Git commit ID: {}\n\n", git_config.commit_ref));

  if verify_result.success() {
    // Get the actual diff content with ANSI colors
    let diff_output = Command::new("git")
      .arg("--bare")
      .arg("--git-dir")
      .arg(&git_config.repo_name)
      .arg("show")
      .arg("--color=always")
      .arg("--pretty=")
      .arg(&git_config.commit_ref)
      .output()
      .await?;
    if diff_output.status.success() {
      let diff_content = String::from_utf8_lossy(&diff_output.stdout);
      output.push_str(&diff_content);
    } else {
      let error_msg = String::from_utf8_lossy(&diff_output.stderr);
      output.push_str(&format!("Error getting diff: {}\n", error_msg));
    }
  } else {
    output.push_str(&format!(
      "Warning: commit {} does not exist in repository\n",
      git_config.commit_ref
    ));
  }

  Ok(output)
}

pub fn extract_git_config_from_env(env_vars: &[(String, String)]) -> Option<GitConfig> {
  let repo_name = env_vars
    .iter()
    .find(|(k, _)| k == "OX_REPO_NAME")?
    .1
    .clone();
  let commit_ref = env_vars
    .iter()
    .find(|(k, _)| k == "OX_REPO_COMMITREF")?
    .1
    .clone();
  let node_name = env_vars
    .iter()
    .find(|(k, _)| k == "OX_NODE_NAME")?
    .1
    .clone();

  let node_group = env_vars
    .iter()
    .find(|(k, _)| k == "OX_NODE_GROUP")
    .map(|(_, v)| v.clone());
  let job_status = env_vars
    .iter()
    .find(|(k, _)| k == "OX_JOB_STATUS")
    .map(|(_, v)| v.clone());
  let job_time = env_vars
    .iter()
    .find(|(k, _)| k == "OX_JOB_TIME")
    .map(|(_, v)| v.clone());

  Some(GitConfig {
    repo_name,
    commit_ref,
    node_name,
    node_group,
    job_status,
    job_time,
  })
}
