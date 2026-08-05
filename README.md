# fak

`fak` fixes mistyped shell commands with an AI model, shows the proposed change, and only executes it after you approve it.

The release binaries contain the LFM2.5 230M GGUF model. Local mode therefore works without Ollama, an external model server, or a separate model download at runtime.

## Features

- Embedded local LFM2.5 model for offline command correction.
- Optional NVIDIA NIM, Cerebras, Gemini, and Groq providers.
- A visible command diff and confirmation prompt before execution.
- Bash, Zsh, and PowerShell history integration.
- Release binaries for Linux, macOS, and Windows on x86_64 and ARM64.
- Rust code compiled with `unsafe` forbidden.

## Download a release

Download the standalone binary or the archive for your operating system and CPU from the [latest release](https://github.com/Ferrb9579/fak/releases/latest).

The standalone files are the actual compiled binaries and are published directly as release assets:

| Platform | Standalone binary | Archive |
| --- | --- | --- |
| Linux x86_64 | `fak-linux-x86_64` | `fak-linux-x86_64.tar.gz` |
| Linux ARM64 | `fak-linux-aarch64` | `fak-linux-aarch64.tar.gz` |
| macOS Intel | `fak-macos-x86_64` | `fak-macos-x86_64.tar.gz` |
| macOS Apple Silicon | `fak-macos-aarch64` | `fak-macos-aarch64.tar.gz` |
| Windows x86_64 | `fak-windows-x86_64.exe` | `fak-windows-x86_64.zip` |
| Windows ARM64 | `fak-windows-aarch64.exe` | `fak-windows-aarch64.zip` |

Every standalone binary and archive has a matching `.sha256` file. Archives also contain `LICENSE-MODEL.txt`.

## Install with one command

The installers download the latest release for your operating system and CPU, verify its SHA-256 checksum, install it for your user, add it to `PATH`, and enable the shell history hook. They do not require Rust, Ollama, or a separate model download.

On Linux or macOS, run this from Bash or Zsh:

```bash
curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/Ferrb9579/fak/master/install.sh | sh
```

Open a new terminal after the installer finishes. It configures `~/.bashrc`, `~/.bash_profile`, `~/.profile`, or `~/.zshrc` as appropriate, so this works immediately in the new terminal:

```bash
git statuss
fak
```

On Windows, run this in PowerShell:

```powershell
irm https://raw.githubusercontent.com/Ferrb9579/fak/master/install.ps1 | iex
```

The Windows installer adds `fak.exe` to your user `PATH` and adds a native `fak` function to your PowerShell profile. Git Bash and WSL users should run the Unix installer from Git Bash or WSL instead.

The installers use the latest published release by default. To install a specific release, set `FAK_VERSION` before running the installer:

```bash
FAK_VERSION=v1.0.2 sh -c 'curl --proto "=https" --tlsv1.2 -sSf https://raw.githubusercontent.com/Ferrb9579/fak/master/install.sh | sh'
```

If you download the Windows archive manually, the executable is inside the ZIP. After extraction, run:

```text
fak-windows-x86_64\fak.exe --help
```

The ARM64 archive contains the same `fak.exe` name under `fak-windows-aarch64`.

## Manual install on Linux or macOS

For example, the following installs the standalone Linux x86_64 binary into `~/.local/bin`:

```bash
curl -LO https://github.com/Ferrb9579/fak/releases/latest/download/fak-linux-x86_64
mkdir -p ~/.local/bin
install -m 0755 fak-linux-x86_64 ~/.local/bin/fak
```

Replace `fak-linux-x86_64` with the matching macOS or ARM64 standalone asset from the table above. The archive form is also available when you want the model license beside the executable:

```bash
curl -LO https://github.com/Ferrb9579/fak/releases/latest/download/fak-linux-x86_64.tar.gz
tar -xzf fak-linux-x86_64.tar.gz
mkdir -p ~/.local/bin
install -m 0755 fak-linux-x86_64/fak ~/.local/bin/fak
```

Make sure `~/.local/bin` is on your `PATH`:

```bash
export PATH="$HOME/.local/bin:$PATH"
fak --help
```

## Manual install on Windows

In PowerShell, you can download the standalone `.exe` directly:

```powershell
Invoke-WebRequest `
  -Uri https://github.com/Ferrb9579/fak/releases/latest/download/fak-windows-x86_64.exe `
  -OutFile fak.exe
.\fak.exe --help
```

Alternatively, download the archive from the release page and extract it:

```powershell
Expand-Archive .\fak-windows-x86_64.zip -DestinationPath .
.\fak-windows-x86_64\fak.exe --help
```

You can move the extracted folder to a permanent location and add that folder to your user `PATH` if you want to run `fak.exe` from anywhere. For automatic history integration, use the one-command installer above. It adds a native PowerShell hook; Git Bash and WSL use the Bash/Zsh hook.

## Verify a download

For a standalone Linux binary:

```bash
sha256sum -c fak-linux-x86_64.sha256
```

For a standalone macOS binary:

```bash
shasum -a 256 -c fak-macos-aarch64.sha256
```

On Windows, compare the output of `Get-FileHash` with the contents of the matching standalone checksum file:

```powershell
(Get-FileHash .\fak-windows-x86_64.exe -Algorithm SHA256).Hash
Get-Content .\fak-windows-x86_64.exe.sha256
```

The archive checksum files use the same commands with the archive filename instead.

## Choose a model provider

Use the embedded model when you do not want an API key or external service:

```bash
export FAK_PROVIDER=local
fak git stats
```

The local provider is bundled in the release binary. It does not require Ollama or a model installation.

Remote providers can be selected explicitly with `FAK_PROVIDER`:

| Provider | Selection | API key | Optional model variable |
| --- | --- | --- | --- |
| Local | `local` or `lfm` | None | — |
| NVIDIA | `nvidia` | `NVIDIA_API_KEY` | `NVIDIA_MODEL` |
| Cerebras | `cerebras` | `CEREBRAS_API_KEY` | `CEREBRAS_MODEL` |
| Gemini | `gemini` | `GEMINI_API_KEY` | `GEMINI_MODEL` |
| Groq | `groq` | `GROQ_API_KEY` | `GROQ_MODEL` |

For example, NVIDIA configuration:

```bash
export FAK_PROVIDER=nvidia
export NVIDIA_API_KEY="your-nvidia-api-key"
fak git stats
```

Default remote models are:

- NVIDIA: `google/gemma-4-31b-it`
- Cerebras: `llama3.1-8b`
- Gemini: `gemma-4-31b-it`
- Groq: `llama-3.3-70b-versatile`

If `FAK_PROVIDER` is not set, `fak` uses the embedded local model. Remote providers are never selected automatically; set `FAK_PROVIDER` explicitly when you want one.

Never commit API keys. For local development, `fak` can read an ignored `.env` file, but normal shell environment variables work as well:

```dotenv
FAK_PROVIDER=local
# FAK_PROVIDER=nvidia
# NVIDIA_API_KEY=replace-me
```

## Automatic shell integration

`fak --alias` prints a shell function that connects `fak` to your command history. The one-command installer adds it automatically. If you install a binary manually, add the evaluation command to your shell startup file.

For Bash, add it to `~/.bashrc`:

```bash
echo 'eval "$(command fak --alias)"' >> ~/.bashrc
source ~/.bashrc
```

For Zsh, add it to `~/.zshrc`:

```zsh
echo 'eval "$(command fak --alias)"' >> ~/.zshrc
source ~/.zshrc
```

Run the append command only once. If the line is already present, just open a new terminal or run `source ~/.bashrc` / `source ~/.zshrc`.

If your Bash terminal is configured as a login shell, place the same line in `~/.bash_profile` or `~/.profile` instead. Git Bash uses the Bash instructions.

To enable it only in the current terminal without editing a startup file:

```bash
eval "$(command fak --alias)"
```

## Uninstall

On Linux or macOS, remove only the installed binary:

```bash
rm -f ~/.local/bin/fak
```

Then remove the exact `eval "$(command fak --alias)"` line from any startup file where the installer added it, such as `~/.bashrc`, `~/.bash_profile`, `~/.profile`, or `~/.zshrc`. Remove the `~/.local/bin` `PATH` line too if it was added only for `fak`. Open a new terminal afterward.

On Windows, the one-command installer uses `$env:LOCALAPPDATA\Programs\fak`. Remove it and remove that directory from your user `PATH`:

```powershell
Remove-Item "$env:LOCALAPPDATA\Programs\fak" -Recurse -Force
```

Also remove the block between `# >>> fak shell integration >>>` and `# <<< fak shell integration <<<` from `$PROFILE`. For Git Bash or WSL, remove the generated hook line from the relevant Bash or Zsh startup file.

## Use it from Bash or Zsh

Now correct the most recent command:

```text
gti status
fak
```

Or provide a command directly:

```bash
fak git stats
```

`fak` will display the proposed correction. Press Enter to execute it, or press Ctrl+C to cancel. The accepted command is added to shell history.

For commands containing pipes, redirects, or other shell syntax, quote the complete command:

```bash
fak 'git status | head'
```

You can generate an alias with another function name if `fak` is already in use:

```bash
echo 'eval "$(command fak --alias=fix)"' >> ~/.bashrc
source ~/.bashrc
fix git stats
```

The same custom-alias command can be placed in `~/.zshrc`. Replace `fix` with the function name you want.

## Safety and privacy

- Review the proposed command before pressing Enter. The generated text is executable shell input.
- Local mode keeps the request on your machine and does not need a network service.
- Remote providers receive the command and prompt through their APIs; check their privacy policies before using them with sensitive commands.
- API keys are read from environment variables and are not embedded in the binary.
- Each release archive includes `LICENSE-MODEL.txt` for the bundled model. Review that license before redistributing the binary.

## Build from source

Release archives are the easiest way to use `fak`. A source build requires Rust stable plus the native C/C++ build tools used by `llama-cpp-2` on your platform.

The model file must exist at this exact path before compiling:

```text
models/LFM2.5-230M.Q4_K_M.gguf
```

Download the pinned `LFM2.5-230M-Q4_K_M.gguf` file from the [LFM2.5 GGUF repository](https://huggingface.co/LiquidAI/LFM2.5-230M-GGUF), rename it to the path above, and verify it against the SHA-256 value in the release workflow before building:

```bash
cargo build --release
```

The resulting binary is `target/release/fak` on Unix-like systems and `target/release/fak.exe` on Windows.

## Troubleshooting

### `FAK_HISTORY` is missing

The shell function has not been loaded. Run:

```bash
eval "$(fak --alias)"
```

Then run `fak` again.

### An API key error appears

An API key is only needed when a remote provider was selected explicitly. Return to the embedded model with:

```bash
unset FAK_PROVIDER
export FAK_PROVIDER=local
```

### The model is missing during a source build

Confirm that the downloaded GGUF file is named exactly:

```text
models/LFM2.5-230M.Q4_K_M.gguf
```
