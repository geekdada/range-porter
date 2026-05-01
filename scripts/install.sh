#!/usr/bin/env bash
# range-porter installer: download, verify, install/upgrade/uninstall.
# Also installs range-porter-manager, an interactive systemd service helper.
# Usage:
#   install.sh [install|upgrade|uninstall] [flags]
# Flags:
#   --version vX.Y.Z      (env RANGE_PORTER_VERSION)     default: latest
#   --install-dir DIR     (env RANGE_PORTER_INSTALL_DIR) default: /usr/local/bin
#   --variant musl|gnu    (env RANGE_PORTER_VARIANT)     default: musl (Linux only)
#   --repo OWNER/NAME     (env RANGE_PORTER_REPO)        default: geekdada/range-porter
#   --manager-skip        skip installing range-porter-manager
#   -h|--help

set -euo pipefail

BIN_NAME="range-porter"
MANAGER_NAME="range-porter-manager"
REPO="${RANGE_PORTER_REPO:-geekdada/range-porter}"
VERSION="${RANGE_PORTER_VERSION:-latest}"
INSTALL_DIR="${RANGE_PORTER_INSTALL_DIR:-/usr/local/bin}"
VARIANT="${RANGE_PORTER_VARIANT:-musl}"
CMD="install"
SKIP_MANAGER=0

log()  { printf '==> %s\n' "$*"; }
warn() { printf 'warning: %s\n' "$*" >&2; }
die()  { printf 'error: %s\n' "$*" >&2; exit 1; }

usage() {
  cat <<'EOF'
range-porter installer: download, verify, install/upgrade/uninstall.
Also installs range-porter-manager, an interactive systemd service helper.

Usage:
  install.sh [install|upgrade|uninstall] [flags]

Flags:
  --version vX.Y.Z      (env RANGE_PORTER_VERSION)     default: latest
  --install-dir DIR     (env RANGE_PORTER_INSTALL_DIR) default: /usr/local/bin
  --variant musl|gnu    (env RANGE_PORTER_VARIANT)     default: musl (Linux only)
  --repo OWNER/NAME     (env RANGE_PORTER_REPO)        default: geekdada/range-porter
  --manager-skip        skip installing range-porter-manager
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
      --manager-skip) SKIP_MANAGER=1; shift ;;
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

generate_manager_script() {
  # Generate range-porter-manager at the target path with embedded defaults.
  local dest="$1"
  cat > "$dest" <<'MANAGER_SCRIPT'
#!/usr/bin/env bash
# range-porter-manager: interactive systemd service helper for range-porter.
# Installed by scripts/install.sh — do not edit manually.
set -euo pipefail

BIN_NAME="range-porter"
SERVICE_NAME="range-porter"
UNIT_FILE="/etc/systemd/system/${SERVICE_NAME}.service"
UNIT_OVERRIDE_DIR="/etc/systemd/system/${SERVICE_NAME}.service.d"

MANAGER_REPO="__REPO__"
MANAGER_INSTALL_DIR="__INSTALL_DIR__"
MANAGER_VARIANT="__VARIANT__"
MANAGER_VERSION="__VERSION__"
DEFAULT_LISTEN_HOST="0.0.0.0"
DEFAULT_UDP_IDLE_TIMEOUT="120s"
DEFAULT_STATS_WINDOW="60"
DEFAULT_SUMMARY_INTERVAL="60s"
DEFAULT_MAX_TCP_CONNECTIONS="65536"

INSTALLER_URL="https://raw.githubusercontent.com/${MANAGER_REPO}/master/scripts/install.sh"

# ---- helpers ----
log()    { printf '==> %s\n' "$*"; }
warn()   { printf 'warning: %s\n' "$*" >&2; }
die()    { printf 'error: %s\n' "$*" >&2; exit 1; }

maybe_sudo() {
  if command -v sudo >/dev/null 2>&1; then
    echo "sudo"
  else
    echo ""
  fi
}

require_systemd() {
  if ! command -v systemctl >/dev/null 2>&1; then
    die "systemctl not found — systemd is required on this host"
  fi
}

has_service() {
  [ -f "$UNIT_FILE" ]
}

# ---- binary upgrade ----
upgrade_binary() {
  log "upgrading $BIN_NAME binary..."
  local sudo_cmd
  sudo_cmd="$(maybe_sudo)"
  curl -fsSL "$INSTALLER_URL" | $sudo_cmd bash -s -- upgrade \
    --repo "$MANAGER_REPO" \
    --install-dir "$MANAGER_INSTALL_DIR" \
    --variant "$MANAGER_VARIANT" \
    --version "$MANAGER_VERSION"
}

# ---- interactive prompts ----
prompt_required() {
  local label="$1"
  local val=""
  while [ -z "$val" ]; do
    read -r -p "$label: " val
  done
  echo "$val"
}

prompt_optional() {
  local label="$1" default="$2"
  local val
  read -r -p "${label} [${default}]: " val
  echo "${val:-$default}"
}

collect_config() {
  # All user-facing output goes to stderr so command substitution
  # only captures the final exec_args line on stdout.
  printf >&2 '\n=== range-porter service configuration ===\n'
  printf >&2 'Press Enter to accept defaults. Required fields (*).\n\n'

  local listen_host listen_ports target dns_server udp_idle
  local stats_bind stats_window summary_interval max_tcp

  listen_host="$(prompt_optional "  Listen host" "$DEFAULT_LISTEN_HOST")"
  listen_ports="$(prompt_required "* Listen ports (e.g. 80,443,10000-10100)")"
  target="$(prompt_required "* Target (e.g. 127.0.0.1:8080)")"
  dns_server="$(prompt_optional "  DNS server (IP:port)" "")"
  udp_idle="$(prompt_optional "  UDP idle timeout" "$DEFAULT_UDP_IDLE_TIMEOUT")"
  stats_bind="$(prompt_optional "  Stats bind (IP:port)" "")"
  stats_window="$(prompt_optional "  Stats window minutes" "$DEFAULT_STATS_WINDOW")"
  summary_interval="$(prompt_optional "  Summary interval" "$DEFAULT_SUMMARY_INTERVAL")"
  max_tcp="$(prompt_optional "  Max TCP connections" "$DEFAULT_MAX_TCP_CONNECTIONS")"

  printf >&2 '\n--- Summary ---\n'
  printf >&2 '  Listen host:      %s\n' "$listen_host"
  printf >&2 '  Listen ports:     %s\n' "$listen_ports"
  printf >&2 '  Target:           %s\n' "$target"
  [ -n "$dns_server" ]      && printf >&2 '  DNS server:       %s\n' "$dns_server"
  printf >&2 '  UDP idle timeout: %s\n' "$udp_idle"
  [ -n "$stats_bind" ]      && printf >&2 '  Stats bind:       %s\n' "$stats_bind"
  printf >&2 '  Stats window:     %s\n' "$stats_window"
  printf >&2 '  Summary interval: %s\n' "$summary_interval"
  printf >&2 '  Max TCP conns:    %s\n' "$max_tcp"
  printf >&2 '\n'

  read -r -p "Proceed with this configuration? [Y/n] " confirm
  case "${confirm:-y}" in
    [Yy]|[Yy][Ee][Ss]) ;;
    *) die "aborted" ;;
  esac

  # Build the ExecStart line — this is the only stdout output.
  local exec_args="--listen-host ${listen_host} --listen-ports ${listen_ports} --target ${target}"
  exec_args="${exec_args} --udp-idle-timeout ${udp_idle}"
  exec_args="${exec_args} --stats-window ${stats_window}"
  exec_args="${exec_args} --summary-interval ${summary_interval}"
  exec_args="${exec_args} --max-tcp-connections ${max_tcp}"
  [ -n "$dns_server" ]      && exec_args="${exec_args} --dns-server ${dns_server}"
  [ -n "$stats_bind" ]      && exec_args="${exec_args} --stats-bind ${stats_bind}"

  echo "$exec_args"
}

# ---- systemd unit ----
write_unit() {
  local exec_args="$1"
  local sudo_cmd
  sudo_cmd="$(maybe_sudo)"

  log "writing systemd unit to $UNIT_FILE"
  $sudo_cmd tee "$UNIT_FILE" > /dev/null <<UNIT_EOF
[Unit]
Description=range-porter TCP/UDP port-range forwarder
Documentation=https://github.com/${MANAGER_REPO}
Wants=network-online.target
After=network-online.target

[Service]
Type=simple
ExecStart=${MANAGER_INSTALL_DIR}/${BIN_NAME} ${exec_args}
Restart=on-failure
RestartSec=2

[Install]
WantedBy=multi-user.target
UNIT_EOF

  $sudo_cmd systemctl daemon-reload
}

# ---- lifecycle commands ----
install_service() {
  require_systemd
  log "installing/updating ${SERVICE_NAME} service..."
  upgrade_binary

  local exec_args
  if [ $# -gt 0 ]; then
    exec_args="$*"
  else
    exec_args="$(collect_config)"
  fi

  write_unit "$exec_args"
  local sudo_cmd
  sudo_cmd="$(maybe_sudo)"
  $sudo_cmd systemctl enable "$SERVICE_NAME"
  $sudo_cmd systemctl restart "$SERVICE_NAME"
  log "service installed and started"
}

uninstall_service() {
  require_systemd
  local sudo_cmd
  sudo_cmd="$(maybe_sudo)"
  log "stopping $SERVICE_NAME"
  $sudo_cmd systemctl stop "$SERVICE_NAME" 2>/dev/null || true
  log "disabling $SERVICE_NAME"
  $sudo_cmd systemctl disable "$SERVICE_NAME" 2>/dev/null || true
  log "removing unit file"
  $sudo_cmd rm -f "$UNIT_FILE"
  $sudo_cmd rm -rf "$UNIT_OVERRIDE_DIR" 2>/dev/null || true
  $sudo_cmd systemctl daemon-reload
  log "service uninstalled"
}

start_service() {
  require_systemd
  log "starting $SERVICE_NAME"
  $(maybe_sudo) systemctl start "$SERVICE_NAME"
}

stop_service() {
  require_systemd
  log "stopping $SERVICE_NAME"
  $(maybe_sudo) systemctl stop "$SERVICE_NAME" || true
}

restart_service() {
  require_systemd
  log "restarting $SERVICE_NAME"
  $(maybe_sudo) systemctl restart "$SERVICE_NAME"
}

show_status() {
  require_systemd
  if has_service; then
    systemctl status "$SERVICE_NAME" --no-pager || true
  else
    warn "service $SERVICE_NAME is not installed"
  fi
}

show_logs() {
  require_systemd
  if has_service; then
    local lines="${1:-50}"
    journalctl -u "$SERVICE_NAME" --no-pager -n "$lines" 2>/dev/null || \
      die "journalctl failed — check that systemd-journald is running"
  else
    warn "service $SERVICE_NAME is not installed"
  fi
}

upgrade_and_restart() {
  require_systemd
  if has_service; then
    upgrade_binary
    restart_service
  else
    warn "service $SERVICE_NAME is not installed — use install-service first"
  fi
}

# ---- interactive menu ----
show_menu() {
  while true; do
    echo
    echo "===================="
    echo " range-porter-mgr"
    echo "===================="
    echo " 1) Install/update service"
    echo " 2) Upgrade binary and restart"
    echo " 3) Start service"
    echo " 4) Stop service"
    echo " 5) Restart service"
    echo " 6) Show status"
    echo " 7) Show logs (last 50 lines)"
    echo " 8) Uninstall service"
    echo " 9) Exit"
    echo "--------------------"
    read -r -p " choice> " choice
    case "$choice" in
      1) install_service ;;
      2) upgrade_and_restart ;;
      3) start_service ;;
      4) stop_service ;;
      5) restart_service ;;
      6) show_status ;;
      7) show_logs 50 ;;
      8)
        read -r -p "Really uninstall systemd service? [y/N] " confirm
        case "${confirm:-n}" in [Yy]|[Yy][Ee][Ss]) uninstall_service ;; *) echo "cancelled" ;; esac
        ;;
      9) echo "bye."; exit 0 ;;
      *) echo "invalid choice" ;;
    esac
  done
}

# ---- main ----
if [ $# -eq 0 ]; then
  show_menu
else
  case "$1" in
    install-service)
      shift
      install_service "$@"
      ;;
    upgrade)
      upgrade_binary
      ;;
    start)
      start_service
      ;;
    stop)
      stop_service
      ;;
    restart)
      restart_service
      ;;
    status)
      show_status
      ;;
    logs)
      show_logs "${2:-50}"
      ;;
    uninstall-service)
      read -r -p "Really uninstall systemd service? [y/N] " confirm
      case "${confirm:-n}" in [Yy]|[Yy][Ee][Ss]) uninstall_service ;; *) die "cancelled" ;; esac
      ;;
    -h|--help)
      cat <<'HELP_EOF'
range-porter-manager: interactive systemd service helper for range-porter.

Usage:
  range-porter-manager                              interactive menu
  range-porter-manager install-service [CLI_ARGS]   install/update service
  range-porter-manager upgrade                      upgrade binary only
  range-porter-manager start|stop|restart|status    service lifecycle
  range-porter-manager logs [N]                     show last N lines (default 50)
  range-porter-manager uninstall-service            remove service
HELP_EOF
      ;;
    *)
      die "unknown command: $1 (use --help)"
      ;;
  esac
fi
MANAGER_SCRIPT

  # Embed installer defaults at generation time.
  sed -i '' \
    -e "s|__REPO__|${REPO}|g" \
    -e "s|__INSTALL_DIR__|${INSTALL_DIR}|g" \
    -e "s|__VARIANT__|${VARIANT}|g" \
    -e "s|__VERSION__|${VERSION}|g" \
    "$dest" 2>/dev/null || \
  sed -i \
    -e "s|__REPO__|${REPO}|g" \
    -e "s|__INSTALL_DIR__|${INSTALL_DIR}|g" \
    -e "s|__VARIANT__|${VARIANT}|g" \
    -e "s|__VERSION__|${VERSION}|g" \
    "$dest"
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

  if [ "$SKIP_MANAGER" -eq 0 ]; then
    log "installing $MANAGER_NAME"
    local manager_tmp
    manager_tmp="$(mktemp)"
    generate_manager_script "$manager_tmp"
    if command -v install >/dev/null 2>&1; then
      $sudo_cmd install -m 0755 "$manager_tmp" "$INSTALL_DIR/$MANAGER_NAME"
    else
      $sudo_cmd cp "$manager_tmp" "$INSTALL_DIR/$MANAGER_NAME"
      $sudo_cmd chmod 0755 "$INSTALL_DIR/$MANAGER_NAME"
    fi
    rm -f "$manager_tmp"
    log "manager installed: $INSTALL_DIR/$MANAGER_NAME"
  fi
}

do_uninstall() {
  local path="$INSTALL_DIR/$BIN_NAME"
  local manager_path="$INSTALL_DIR/$MANAGER_NAME"
  local sudo_cmd
  sudo_cmd="$(maybe_sudo "$INSTALL_DIR")"
  local removed=0

  if [ -e "$path" ]; then
    log "removing $path"
    $sudo_cmd rm -f "$path"
    removed=1
  else
    log "$path not found — skipping"
  fi

  if [ -e "$manager_path" ]; then
    log "removing $manager_path"
    $sudo_cmd rm -f "$manager_path"
    removed=1
  fi

  if [ "$removed" -eq 0 ]; then
    log "nothing to uninstall"
  else
    log "uninstalled"
  fi
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
