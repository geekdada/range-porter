#!/usr/bin/env bash
# range-porter installer: download, verify, install/upgrade/uninstall.
# Usage:
#   install.sh [install|upgrade|uninstall] [flags]
# Flags:
#   --version vX.Y.Z      (env RANGE_PORTER_VERSION)     default: latest
#   --install-dir DIR     (env RANGE_PORTER_INSTALL_DIR) default: /usr/local/bin
#   --variant musl|gnu    (env RANGE_PORTER_VARIANT)     default: musl (Linux only)
#   --repo OWNER/NAME     (env RANGE_PORTER_REPO)        default: geekdada/range-porter
#   -h|--help

set -euo pipefail

BIN_NAME="range-porter"
REPO="${RANGE_PORTER_REPO:-geekdada/range-porter}"
VERSION="${RANGE_PORTER_VERSION:-latest}"
INSTALL_DIR="${RANGE_PORTER_INSTALL_DIR:-/usr/local/bin}"
VARIANT="${RANGE_PORTER_VARIANT:-musl}"
CMD="install"

log()  { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'EOF'
range-porter installer: download, verify, install/upgrade/uninstall.

Usage:
  install.sh [install|upgrade|uninstall] [flags]

Flags:
  --version vX.Y.Z      (env RANGE_PORTER_VERSION)     default: latest
  --install-dir DIR     (env RANGE_PORTER_INSTALL_DIR) default: /usr/local/bin
  --variant musl|gnu    (env RANGE_PORTER_VARIANT)     default: musl (Linux only)
  --repo OWNER/NAME     (env RANGE_PORTER_REPO)        default: geekdada/range-porter
  -h, --help            show this help
EOF
}

parse_args() {
  if [ $# -gt 0 ]; then
    case "$1" in
      install|upgrade|uninstall)
        CMD="$1"; shift ;;
      -h|--help)
        usage; exit 0 ;;
    esac
  fi

  while [ $# -gt 0 ]; do
    case "$1" in
      --version)      VERSION="${2:?--version requires a value}"; shift 2 ;;
      --version=*)    VERSION="${1#*=}"; shift ;;
      --install-dir)  INSTALL_DIR="${2:?--install-dir requires a value}"; shift 2 ;;
      --install-dir=*) INSTALL_DIR="${1#*=}"; shift ;;
      --variant)      VARIANT="${2:?--variant requires a value}"; shift 2 ;;
      --variant=*)    VARIANT="${1#*=}"; shift ;;
      --repo)         REPO="${2:?--repo requires a value}"; shift 2 ;;
      --repo=*)       REPO="${1#*=}"; shift ;;
      -h|--help)      usage; exit 0 ;;
      *)              die "unknown argument: $1 (use --help)" ;;
    esac
  done

  case "$VARIANT" in
    musl|gnu) ;;
    *) die "--variant must be 'musl' or 'gnu', got: $VARIANT" ;;
  esac
}

detect_target() {
  local os arch
  os="$(uname -s)"
  arch="$(uname -m)"
  case "$os" in
    Linux)
      case "$arch" in
        x86_64)           echo "x86_64-unknown-linux-${VARIANT}" ;;
        aarch64|arm64)    echo "aarch64-unknown-linux-${VARIANT}" ;;
        *) die "unsupported Linux arch: $arch (supported: x86_64, aarch64)" ;;
      esac
      ;;
    Darwin)
      case "$arch" in
        x86_64)           echo "x86_64-apple-darwin" ;;
        arm64|aarch64)    echo "aarch64-apple-darwin" ;;
        *) die "unsupported macOS arch: $arch (supported: x86_64, arm64)" ;;
      esac
      ;;
    *) die "unsupported OS: $os (supported: Linux, Darwin)" ;;
  esac
}

require_cmd() {
  command -v "$1" >/dev/null 2>&1 || die "required command not found: $1"
}

sha256_check() {
  # Read a single line "<hex>  <filename>" from stdin and verify in cwd.
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum -c -
  elif command -v shasum >/dev/null 2>&1; then
    shasum -a 256 -c -
  else
    die "need sha256sum or shasum to verify checksum"
  fi
}

resolve_tag() {
  if [ "$VERSION" != "latest" ]; then
    echo "$VERSION"
    return
  fi
  local api="https://api.github.com/repos/${REPO}/releases/latest"
  # Extract "tag_name": "vX.Y.Z" without jq.
  # Buffer the response first so curl doesn't hit SIGPIPE when grep -m1 closes
  # the pipe early ("curl: (23) Failed writing body").
  local body tag
  body="$(curl -fsSL "$api")"
  tag="$(printf '%s\n' "$body" \
    | grep -m1 '"tag_name"' \
    | sed -E 's/.*"tag_name":[[:space:]]*"([^"]+)".*/\1/')"
  [ -n "$tag" ] || die "failed to resolve latest release tag from $api"
  echo "$tag"
}

maybe_sudo() {
  # Print "sudo" if we need it to write to install dir, otherwise empty.
  local dir="$1"
  if [ -d "$dir" ] && [ -w "$dir" ]; then
    echo ""
  elif [ ! -e "$dir" ] && [ -w "$(dirname "$dir")" ]; then
    echo ""
  else
    if command -v sudo >/dev/null 2>&1; then
      echo "sudo"
    else
      die "no write access to $dir and sudo not available"
    fi
  fi
}

do_install() {
  require_cmd curl
  require_cmd tar

  local target tag archive url_base tmp sudo_cmd
  target="$(detect_target)"
  tag="$(resolve_tag)"
  archive="${BIN_NAME}-${tag}-${target}.tar.gz"
  url_base="https://github.com/${REPO}/releases/download/${tag}"

  log "repo:        $REPO"
  log "version:     $tag"
  log "target:      $target"
  log "install dir: $INSTALL_DIR"

  tmp="$(mktemp -d)"
  trap "rm -rf -- '$tmp'" EXIT

  log "downloading $archive"
  curl -fsSL --retry 3 -o "$tmp/$archive" "$url_base/$archive"

  log "downloading SHA256SUMS"
  curl -fsSL --retry 3 -o "$tmp/SHA256SUMS" "$url_base/SHA256SUMS"

  log "verifying checksum"
  ( cd "$tmp" && grep -E "[[:space:]]${archive}$" SHA256SUMS | sha256_check ) \
    || die "checksum verification failed for $archive"

  log "extracting"
  tar -xzf "$tmp/$archive" -C "$tmp"

  local extracted
  extracted="$(find "$tmp" -type f -name "$BIN_NAME" -perm -u+x | head -n1)"
  [ -n "$extracted" ] || die "binary '$BIN_NAME' not found inside archive"

  sudo_cmd="$(maybe_sudo "$INSTALL_DIR")"
  if [ ! -d "$INSTALL_DIR" ]; then
    log "creating $INSTALL_DIR"
    $sudo_cmd mkdir -p "$INSTALL_DIR"
  fi

  log "installing to $INSTALL_DIR/$BIN_NAME"
  if command -v install >/dev/null 2>&1; then
    $sudo_cmd install -m 0755 "$extracted" "$INSTALL_DIR/$BIN_NAME"
  else
    $sudo_cmd cp "$extracted" "$INSTALL_DIR/$BIN_NAME"
    $sudo_cmd chmod 0755 "$INSTALL_DIR/$BIN_NAME"
  fi

  log "installed: $($INSTALL_DIR/$BIN_NAME --version 2>/dev/null || echo "$INSTALL_DIR/$BIN_NAME")"
  case ":${PATH}:" in
    *":${INSTALL_DIR}:"*) ;;
    *) warn "$INSTALL_DIR is not in your PATH — add it to use '$BIN_NAME' directly." ;;
  esac
}

do_uninstall() {
  local path="$INSTALL_DIR/$BIN_NAME"
  if [ ! -e "$path" ]; then
    log "nothing to uninstall: $path not found"
    return 0
  fi
  local sudo_cmd
  sudo_cmd="$(maybe_sudo "$INSTALL_DIR")"
  log "removing $path"
  $sudo_cmd rm -f "$path"
  log "uninstalled"
}

main() {
  parse_args "$@"
  case "$CMD" in
    install|upgrade) do_install ;;
    uninstall)       do_uninstall ;;
    *) die "unknown command: $CMD" ;;
  esac
}

main "$@"
