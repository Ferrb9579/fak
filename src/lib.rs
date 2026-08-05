#![forbid(unsafe_code)]

pub mod cerebras;
mod local_llama;
pub mod nvidia;

use std::env;
use std::path::Path;
use std::process::{Command, ExitStatus, Stdio};

use rig::client::CompletionClient;
use rig::completion::Prompt;

const DIFF_WITH_ALIAS: f64 = 0.5;

fn binary_path() -> String {
    env::current_exe()
        .ok()
        .map(|p| p.display().to_string())
        .unwrap_or_else(|| "fak".to_string())
}

fn shell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "'\\''"))
}

fn fish_quote(value: &str) -> String {
    format!("'{}'", value.replace('\\', "\\\\").replace('\'', "\\'"))
}

fn powershell_quote(value: &str) -> String {
    format!("'{}'", value.replace('\'', "''"))
}

fn nushell_quote(value: &str) -> String {
    let mut hashes = 1;
    loop {
        let marker = "#".repeat(hashes);
        if !value.contains(&format!("'{}", marker)) {
            return format!("r{marker}'{value}'{marker}");
        }
        hashes += 1;
    }
}

fn safe_alias_name(alias_name: &str) -> &str {
    if !alias_name.is_empty()
        && alias_name
            .chars()
            .all(|character| character.is_ascii_alphanumeric() || character == '_')
        && !alias_name
            .chars()
            .next()
            .is_some_and(|character| character.is_ascii_digit())
    {
        alias_name
    } else {
        "fak"
    }
}

fn shell_name(value: &str) -> String {
    let name = value.rsplit(['/', '\\']).next().unwrap_or(value);
    let lower = name.to_ascii_lowercase();
    lower.trim_end_matches(".exe").to_string()
}

fn is_shell_name(name: &str) -> bool {
    matches!(
        name,
        "ash"
            | "bash"
            | "cmd"
            | "csh"
            | "dash"
            | "fish"
            | "ksh"
            | "mksh"
            | "nu"
            | "nushell"
            | "powershell"
            | "pwsh"
            | "sh"
            | "tcsh"
            | "zsh"
    )
}

#[cfg(unix)]
fn parent_shell() -> Option<String> {
    let mut pid = std::process::id().to_string();

    for _ in 0..8 {
        let output = Command::new("ps")
            .args(["-p", &pid, "-o", "ppid=,comm="])
            .output()
            .ok()?;
        let stdout = String::from_utf8_lossy(&output.stdout);
        let mut fields = stdout.split_whitespace();
        let parent_pid = fields.next()?.to_string();
        let command = fields.next()?;
        let name = shell_name(command);

        if is_shell_name(&name) {
            return Some(name);
        }
        if parent_pid == pid || parent_pid == "0" {
            break;
        }
        pid = parent_pid;
    }

    None
}

pub fn app_alias(alias_name: &str) -> String {
    let shell = detect_shell();
    app_alias_for_shell(alias_name, &shell, &binary_path())
}

fn app_alias_for_shell(alias_name: &str, shell: &str, bin: &str) -> String {
    let name = safe_alias_name(alias_name);

    match shell {
        "zsh" => format!(
            r#"
{name} () {{
    export FAK_SHELL=zsh;
    export FAK_ALIAS={name};
    export FAK_HISTORY="$(fc -ln -10)";
    local _fak_fixed
    _fak_fixed="$({bin} --shell-command "$@")"
    local _fak_status=$?
    unset FAK_HISTORY FAK_SHELL FAK_ALIAS;
    if (( _fak_status != 0 || -z "$_fak_fixed" )); then
        return $_fak_status
    fi
    print -s -- "$_fak_fixed"
    eval "$_fak_fixed"
}}
"#,
            name = name,
            bin = shell_quote(bin)
        ),
        "fish" => format!(
            r#"
function {name}
    set -lx FAK_SHELL fish
    set -lx FAK_ALIAS '{name}'
    set -lx FAK_HISTORY (history --reverse --max=10 | string collect)
    set -l _fak_fixed ({bin} --shell-command $argv | string collect)
    set -l _fak_status $pipestatus[1]
    if test $_fak_status -ne 0; or test -z "$_fak_fixed"
        return $_fak_status
    end
    if status is-interactive
        commandline --replace -- "$_fak_fixed"
        commandline --function execute
    else
        eval "$_fak_fixed"
    end
end
"#,
            name = name,
            bin = fish_quote(bin)
        ),
        "nu" | "nushell" => format!(
            r#"
def --wrapped {name} [...args] {{
    let history_text = (history | last 10 | get command | str join (char newline))
    let fixed = (with-env {{
        FAK_SHELL: "nu",
        FAK_ALIAS: "{name}",
        FAK_HISTORY: $history_text,
    }} {{
        ^{bin} --shell-command ...$args
    }} | str trim)
    if ($fixed | is-empty) {{
        return
    }}
    $fixed | history import
    ^nu -c $fixed
}}
"#,
            name = name,
            bin = nushell_quote(bin)
        ),
        "powershell" | "pwsh" | "powershell.exe" | "pwsh.exe" => format!(
            r#"
function {name} {{
    $oldHistory = $env:FAK_HISTORY
    $oldShell = $env:FAK_SHELL
    $oldAlias = $env:FAK_ALIAS
    try {{
        $env:FAK_SHELL = 'powershell'
        $env:FAK_ALIAS = '{name}'
        $historyLines = @(Get-History -Count 10 | ForEach-Object {{ $_.CommandLine }})
        $env:FAK_HISTORY = ($historyLines -join [Environment]::NewLine)
        $fixedOutput = & {bin} --shell-command @args
        $status = $LASTEXITCODE
        $fixed = ($fixedOutput -join [Environment]::NewLine).Trim()
        if ($status -ne 0 -or [string]::IsNullOrWhiteSpace($fixed)) {{
            return
        }}
        try {{
            [Microsoft.PowerShell.PSConsoleReadLine]::AddToHistory($fixed)
        }} catch {{
            # PSReadLine is optional; command execution still works without it.
        }}
        Invoke-Expression $fixed
    }} finally {{
        if ($null -eq $oldHistory) {{ Remove-Item Env:FAK_HISTORY -ErrorAction SilentlyContinue }} else {{ $env:FAK_HISTORY = $oldHistory }}
        if ($null -eq $oldShell) {{ Remove-Item Env:FAK_SHELL -ErrorAction SilentlyContinue }} else {{ $env:FAK_SHELL = $oldShell }}
        if ($null -eq $oldAlias) {{ Remove-Item Env:FAK_ALIAS -ErrorAction SilentlyContinue }} else {{ $env:FAK_ALIAS = $oldAlias }}
    }}
}}
"#,
            name = name,
            bin = powershell_quote(bin)
        ),
        _ => format!(
            r#"
function {name} () {{
    export FAK_SHELL=bash;
    export FAK_ALIAS={name};
    export FAK_HISTORY=$(fc -ln -10);
    local _fak_fixed
    _fak_fixed="$({bin} --shell-command "$@")"
    local _fak_status=$?
    unset FAK_HISTORY FAK_SHELL FAK_ALIAS;
    if [ "$_fak_status" -ne 0 ] || [ -z "$_fak_fixed" ]; then
        return "$_fak_status"
    fi
    history -s "$_fak_fixed"
    eval "$_fak_fixed"
}}
"#,
            name = name,
            bin = shell_quote(bin)
        ),
    }
}

pub fn detect_shell() -> String {
    if let Ok(shell) = env::var("FAK_SHELL") {
        return shell_name(&shell);
    }
    #[cfg(unix)]
    if let Some(shell) = parent_shell() {
        return shell;
    }
    if let Some(shell) = env::var("SHELL").ok().and_then(|shell| {
        Path::new(&shell)
            .file_name()
            .and_then(|name| name.to_str())
            .map(shell_name)
    }) {
        return shell;
    }

    if cfg!(windows) {
        "powershell".to_string()
    } else {
        "bash".to_string()
    }
}

pub fn execute_command(command: &str) -> std::io::Result<ExitStatus> {
    let shell = detect_shell();
    let mut process = match shell.as_str() {
        "powershell" | "powershell.exe" => Command::new(if cfg!(windows) {
            "powershell.exe"
        } else {
            "pwsh"
        }),
        "pwsh" | "pwsh.exe" => Command::new("pwsh"),
        "cmd" | "cmd.exe" => {
            let mut process = Command::new("cmd.exe");
            process.args(["/D", "/S", "/C"]);
            process
        }
        "fish" => Command::new("fish"),
        "nu" | "nushell" => Command::new("nu"),
        _ => {
            let program = env::var("SHELL").unwrap_or_else(|_| shell.clone());
            Command::new(program)
        }
    };

    match shell.as_str() {
        "powershell" | "powershell.exe" | "pwsh" | "pwsh.exe" => {
            process.args(["-NoProfile", "-Command", command]);
        }
        "cmd" | "cmd.exe" => {
            process.arg(command);
        }
        _ => {
            process.args(["-c", command]);
        }
    }

    process
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
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

#[cfg(test)]
mod tests {
    use super::app_alias_for_shell;

    #[test]
    fn renders_a_fish_history_hook() {
        let alias = app_alias_for_shell("fak", "fish", "/tmp/fak");

        assert!(alias.contains("set -lx FAK_SHELL fish"));
        assert!(alias.contains("history --reverse --max=10"));
        assert!(alias.contains("commandline --function execute"));
    }

    #[test]
    fn renders_a_powershell_history_hook() {
        let alias = app_alias_for_shell("fak", "powershell", "C:\\Tools\\fak.exe");

        assert!(alias.contains("$env:FAK_SHELL = 'powershell'"));
        assert!(alias.contains("Get-History -Count 10"));
        assert!(alias.contains("Invoke-Expression $fixed"));
        assert!(alias.contains("'C:\\Tools\\fak.exe'"));
    }

    #[test]
    fn renders_a_nushell_history_hook() {
        let alias = app_alias_for_shell("fak", "nu", "/tmp/fak");

        assert!(alias.contains("def --wrapped fak [...args]"));
        assert!(alias.contains("history | last 10 | get command"));
        assert!(alias.contains("FAK_SHELL: \"nu\""));
        assert!(alias.contains("^r#'/tmp/fak'# --shell-command ...$args"));
    }

    #[test]
    fn invalid_alias_names_fall_back_to_fak() {
        let alias = app_alias_for_shell("bad-name", "fish", "/tmp/fak");

        assert!(alias.contains("function fak"));
        assert!(!alias.contains("function bad-name"));
    }
}
