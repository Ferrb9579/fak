#!/bin/sh

# One-command installer for Linux, macOS, and Git Bash/MSYS2.
#
# Usage:
#   curl --proto '=https' --tlsv1.2 -sSf https://raw.githubusercontent.com/Ferrb9579/fak/master/install.sh | sh

set -eu

REPOSITORY="Ferrb9579/fak"
HOME_DIR=${HOME:-}

say() {
    printf '%s\n' "$*"
}

warn() {
    printf 'warning: %s\n' "$*" >&2
}

die() {
    printf 'error: %s\n' "$*" >&2
    exit 1
}

[ -n "$HOME_DIR" ] || die "HOME is not set"
command -v curl >/dev/null 2>&1 || die "curl is required"
command -v uname >/dev/null 2>&1 || die "uname is required"
command -v mktemp >/dev/null 2>&1 || die "mktemp is required"

install_dir=${FAK_INSTALL_DIR:-"$HOME_DIR/.local/bin"}
case "$install_dir" in
    /*) ;;
    *) die "FAK_INSTALL_DIR must be an absolute path" ;;
esac
kernel=$(uname -s)
machine=$(uname -m)

case "$kernel" in
    Linux)
        platform=linux
        ;;
    Darwin)
        platform=macos
        ;;
    MINGW*|MSYS*|CYGWIN*)
        platform=windows
        ;;
    *)
        die "unsupported operating system: $kernel"
        ;;
esac

case "$machine" in
    x86_64|amd64|AMD64)
        architecture=x86_64
        ;;
    aarch64|arm64|ARM64)
        architecture=aarch64
        ;;
    *)
        die "unsupported CPU architecture: $machine"
        ;;
esac

asset="fak-${platform}-${architecture}"
installed_name=fak
if [ "$platform" = windows ]; then
    asset="${asset}.exe"
    installed_name=fak.exe
fi

release=${FAK_VERSION:-latest}
if [ "$release" != latest ] && [ "${release#v}" = "$release" ]; then
    release="v${release}"
fi

if [ "$release" = latest ]; then
    download_base="https://github.com/${REPOSITORY}/releases/latest/download"
else
    download_base="https://github.com/${REPOSITORY}/releases/download/${release}"
fi

temporary_dir=$(mktemp -d 2>/dev/null || mktemp -d -t fak-install)
cleanup() {
    rm -rf "$temporary_dir"
}
trap cleanup EXIT INT TERM

download() {
    url=$1
    destination=$2
    curl --proto '=https' --tlsv1.2 --fail --silent --show-error \
        --location --retry 5 --retry-delay 1 \
        --user-agent fak-installer \
        --output "$destination" "$url"
}

binary_path="$temporary_dir/$asset"
checksum_path="$temporary_dir/$asset.sha256"

say "Downloading fak for ${platform}/${architecture}..."
download "${download_base}/${asset}" "$binary_path"
download "${download_base}/${asset}.sha256" "$checksum_path"

expected_checksum=$(awk 'NF { print $1; exit }' "$checksum_path")
[ -n "$expected_checksum" ] || die "the release checksum is empty"

if command -v sha256sum >/dev/null 2>&1; then
    actual_checksum=$(sha256sum "$binary_path" | awk '{ print $1 }')
elif command -v shasum >/dev/null 2>&1; then
    actual_checksum=$(shasum -a 256 "$binary_path" | awk '{ print $1 }')
else
    die "sha256sum or shasum is required to verify the download"
fi

if [ "$expected_checksum" != "$actual_checksum" ]; then
    die "checksum verification failed for $asset"
fi

mkdir -p "$install_dir"
staged_path="$temporary_dir/$installed_name"
cp "$binary_path" "$staged_path"
if [ "$platform" != windows ]; then
    chmod 0755 "$staged_path"
fi
mv -f "$staged_path" "$install_dir/$installed_name"
binary="$install_dir/$installed_name"

shell_name=${SHELL##*/}
if [ "$install_dir" = "$HOME_DIR/.local/bin" ]; then
    path_line='export PATH="$HOME/.local/bin:$PATH"'
else
    path_line="export PATH='$install_dir':"\$PATH""
fi
hook_line='eval "$(command fak --alias)"'

ensure_line() {
    file=$1
    line=$2

    mkdir -p "$(dirname "$file")"
    [ -f "$file" ] || : > "$file"
    if ! grep -Fqx "$line" "$file" 2>/dev/null; then
        if [ -s "$file" ]; then
            printf '\n' >> "$file"
        fi
        printf '%s\n' "$line" >> "$file"
    fi
}

setup_shell_file() {
    file=$1
    if ensure_line "$file" "$path_line" && \
       (grep -Fq 'fak --alias' "$file" 2>/dev/null || ensure_line "$file" "$hook_line"); then
        say "Shell integration added to $file"
    else
        warn "could not update $file; add these lines manually:"
        say "  $path_line"
        say "  $hook_line"
    fi
}

if [ "${FAK_NO_SHELL_SETUP:-0}" = 1 ]; then
    say "Skipping shell startup changes because FAK_NO_SHELL_SETUP=1"
elif [ "$shell_name" = zsh ]; then
    setup_shell_file "$HOME_DIR/.zshrc"
elif [ "$shell_name" = bash ]; then
    # Cover both interactive and login Bash shells when .bash_profile does not
    # already source .bashrc. This avoids the common "works in one terminal"
    # setup problem without duplicating the hook when the files are linked.
    if [ -f "$HOME_DIR/.bash_profile" ] && \
       ! grep -Eq '(^|[[:space:];])([.]|source)[[:space:]]+.*[.]bashrc' "$HOME_DIR/.bash_profile" 2>/dev/null; then
        setup_shell_file "$HOME_DIR/.bashrc"
        setup_shell_file "$HOME_DIR/.bash_profile"
    elif [ -f "$HOME_DIR/.bashrc" ]; then
        setup_shell_file "$HOME_DIR/.bashrc"
    elif [ -f "$HOME_DIR/.profile" ]; then
        setup_shell_file "$HOME_DIR/.profile"
    else
        setup_shell_file "$HOME_DIR/.bashrc"
    fi
else
    warn "the installer detected '$shell_name'; automatic setup is available for Bash and Zsh"
    say "Add these lines to your shell startup file:"
    say "  $path_line"
    say "  $hook_line"
fi

"$binary" --help >/dev/null

say ""
say "fak was installed at $binary"
say "Open a new terminal, then try:"
say "  git statuss"
say "  fak"
