#![forbid(unsafe_code)]

pub mod cerebras;
mod local_llama;
pub mod nvidia;

use std::env;
use std::path::Path;

use rig::client::CompletionClient;
use rig::completion::Prompt;

const DIFF_WITH_ALIAS: f64 = 0.5;

fn binary_path() -> String {
    env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "fak".to_string())
        .replace('\'', "'\\''")
}

pub fn app_alias(alias_name: &str) -> String {
    let bin = binary_path();

    match detect_shell().as_str() {
        "zsh" => format!(
            r#"
{name} () {{
    export FAK_SHELL=zsh;
    export FAK_ALIAS={name};
    export FAK_HISTORY="$(fc -ln -10)";
    local _fak_fixed
    _fak_fixed="$('{bin}' --shell-command "$@")"
    local _fak_status=$?
    unset FAK_HISTORY FAK_SHELL FAK_ALIAS;
    if (( _fak_status != 0 || -z "$_fak_fixed" )); then
        return $_fak_status
    fi
    print -s -- "$_fak_fixed"
    eval "$_fak_fixed"
}}
"#,
            name = alias_name,
            bin = bin
        ),
        _ => format!(
            r#"
function {name} () {{
    export FAK_SHELL=bash;
    export FAK_ALIAS={name};
    export FAK_HISTORY=$(fc -ln -10);
    local _fak_fixed
    _fak_fixed="$('{bin}' --shell-command "$@")"
    local _fak_status=$?
    unset FAK_HISTORY FAK_SHELL FAK_ALIAS;
    if [ "$_fak_status" -ne 0 ] || [ -z "$_fak_fixed" ]; then
        return "$_fak_status"
    fi
    history -s "$_fak_fixed"
    eval "$_fak_fixed"
}}
"#,
            name = alias_name,
            bin = bin
        ),
    }
}

pub fn detect_shell() -> String {
    if let Ok(shell) = env::var("FAK_SHELL") {
        return shell;
    }
    env::var("SHELL")
        .ok()
        .and_then(|s| {
            Path::new(&s)
                .file_name()
                .and_then(|n| n.to_str())
                .map(|s| s.to_string())
        })
        .unwrap_or_else(|| "bash".to_string())
}

pub fn last_command(forced: &[String]) -> Result<String, String> {
    if !forced.is_empty() {
        return Ok(forced.join(" "));
    }

    let history = env::var("FAK_HISTORY").map_err(|_| {
        "FAK_HISTORY is not set.\n\
         \n\
         `cargo run` / bare `fak` cannot see shell history by itself.\n\
         Load the hook in this shell, then call the function:\n\
         \n\
           eval \"$(cargo run --quiet -- --alias)\"\n\
           fak\n\
         \n\
         (After install: eval \"$(fak --alias)\" in your ~/.bashrc)"
            .to_string()
    })?;

    let alias = env::var("FAK_ALIAS").unwrap_or_else(|_| "fak".to_string());

    for command in history.lines().rev() {
        let command = command.trim();
        if command.is_empty() {
            continue;
        }
        if similarity(&alias, command) >= DIFF_WITH_ALIAS {
            continue;
        }
        return Ok(command.to_string());
    }

    Err("no previous command found in FAK_HISTORY".to_string())
}

fn similarity(a: &str, b: &str) -> f64 {
    if a == b {
        return 1.0;
    }
    if b.split_whitespace().next() == Some(a) {
        return 1.0;
    }

    let (shorter, longer) = if a.len() <= b.len() { (a, b) } else { (b, a) };
    if longer.is_empty() {
        return 1.0;
    }
    let mut matches = 0usize;
    for (ca, cb) in shorter.chars().zip(longer.chars()) {
        if ca == cb {
            matches += 1;
        }
    }
    (2.0 * matches as f64) / ((a.len() + b.len()) as f64)
}

enum ProviderType {
    Nvidia,
    Gemini,
    Groq,
    Cerebras,
    Local,
}

fn detect_provider() -> ProviderType {
    if let Ok(provider) = env::var("FAK_PROVIDER") {
        match provider.to_lowercase().as_str() {
            "nvidia" => ProviderType::Nvidia,
            "groq" => ProviderType::Groq,
            "cerebras" => ProviderType::Cerebras,
            "lfm" | "local" => ProviderType::Local,
            _ => ProviderType::Gemini,
        }
    } else {
        ProviderType::Local
    }
}

pub async fn fix_command(cmd: &str) -> Result<String, String> {
    let preamble = "You are a command-line correction assistant. \
                    The user will give you a misspelled or incorrect CLI command. \
                    Respond ONLY with the corrected single command string, with no markdown formatting, backticks, or explanation.";

    let response = match detect_provider() {
        ProviderType::Nvidia => {
            let api_key = env::var("NVIDIA_API_KEY")
                .map_err(|_| "NVIDIA_API_KEY environment variable is not set".to_string())?;

            let client = nvidia::NvidiaClient::new(api_key);
            let model =
                env::var("NVIDIA_MODEL").unwrap_or_else(|_| "google/gemma-4-31b-it".to_string());
            let agent = client.agent(model).preamble(preamble).build();
            agent
                .prompt(cmd)
                .await
                .map_err(|e| format!("Nvidia request failed: {e}"))?
        }
        ProviderType::Cerebras => {
            let api_key = env::var("CEREBRAS_API_KEY")
                .map_err(|_| "CEREBRAS_API_KEY environment variable is not set".to_string())?;

            let client = cerebras::CerebrasClient::new(api_key);
            let model = env::var("CEREBRAS_MODEL").unwrap_or_else(|_| "llama3.1-8b".to_string());
            let agent = client.agent(model).preamble(preamble).build();
            agent
                .prompt(cmd)
                .await
                .map_err(|e| format!("Cerebras request failed: {e}"))?
        }
        ProviderType::Gemini => {
            let api_key = env::var("GEMINI_API_KEY")
                .map_err(|_| "GEMINI_API_KEY environment variable is not set".to_string())?;

            let client = rig::providers::gemini::Client::new(&api_key)
                .map_err(|e| format!("Failed to create Gemini client: {e}"))?;
            let model = env::var("GEMINI_MODEL").unwrap_or_else(|_| "gemma-4-31b-it".to_string());
            let agent = client.agent(model).preamble(preamble).build();
            agent
                .prompt(cmd)
                .await
                .map_err(|e| format!("Gemini request failed: {e}"))?
        }
        ProviderType::Groq => {
            let api_key = env::var("GROQ_API_KEY")
                .map_err(|_| "GROQ_API_KEY environment variable is not set".to_string())?;

            let client = rig::providers::groq::Client::new(&api_key)
                .map_err(|e| format!("Failed to create Groq client: {e}"))?;
            let model =
                env::var("GROQ_MODEL").unwrap_or_else(|_| "llama-3.3-70b-versatile".to_string());
            let agent = client.agent(model).preamble(preamble).build();
            agent
                .prompt(cmd)
                .await
                .map_err(|e| format!("Groq request failed: {e}"))?
        }
        ProviderType::Local => {
            let local_preamble = "You are a command-line correction assistant. \
                                  Correct the misspelled or incorrect CLI command. \
                                  Respond ONLY with the corrected single command string. Do not use markdown, backticks, or explanations.";
            local_llama::fix_command(cmd, local_preamble)?
        }
    };

    let parsed = if let Some(start) = response.find('`') {
        if let Some(end) = response[start + 1..].find('`') {
            response[start + 1..start + 1 + end].to_string()
        } else {
            response
        }
    } else {
        response
    };

    let mut fixed = parsed.trim().trim_matches('`').trim().to_string();
    let lower = fixed.to_lowercase();
    if let Some(idx) = lower.find("output:") {
        fixed = fixed[idx + "output:".len()..].trim().to_string();
    }

    Ok(fixed)
}

pub fn show_diff(original: &str, corrected: &str) -> String {
    let diff = similar::TextDiff::from_chars(original, corrected);
    let mut result = String::new();

    for change in diff.iter_all_changes() {
        match change.tag() {
            similar::ChangeTag::Delete => {
                // Red for deleted characters
                result.push_str(&format!("\x1b[31m{}\x1b[0m", change.value()));
            }
            similar::ChangeTag::Insert => {
                // Green for inserted characters
                result.push_str(&format!("\x1b[32m{}\x1b[0m", change.value()));
            }
            similar::ChangeTag::Equal => {
                result.push_str(change.value());
            }
        }
    }
    result
}
