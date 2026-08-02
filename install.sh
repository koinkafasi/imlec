#!/usr/bin/env bash
# imlec installer for Linux.
#   curl -fsSL https://raw.githubusercontent.com/koinkafasi/imlec/main/install.sh | bash
set -euo pipefail

REPO="koinkafasi/imlec"
BIN_DIR="${HOME}/.local/bin"
UNIT_DIR="${HOME}/.config/systemd/user"
TMP="$(mktemp -d)"
trap 'rm -rf "${TMP}"' EXIT

info() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m warn\033[0m %s\n' "$*"; }
die()  { printf '\033[1;31merror\033[0m %s\n' "$*" >&2; exit 1; }

ask() {
  # curl | bash leaves stdin consumed by the script, so read from the terminal.
  local prompt="$1" reply=""
  if [ -r /dev/tty ]; then
    read -r -p "${prompt} [y/N] " reply < /dev/tty || true
  fi
  [[ "${reply}" =~ ^[Yy]$ ]]
}

[ "$(uname -s)" = "Linux" ] || die "this installer is for Linux; use install.ps1 on Windows"
[ "$(uname -m)" = "x86_64" ] || die "only x86_64 prebuilt binaries exist; build from source with 'cargo build --release'"

install_from_release() {
  local url
  url="$(curl -fsSL "https://api.github.com/repos/${REPO}/releases/latest" \
    | grep -o '"browser_download_url": *"[^"]*imlec-x86_64-linux.tar.gz"' \
    | head -n1 | cut -d'"' -f4)" || return 1
  [ -n "${url}" ] || return 1
  info "downloading ${url}"
  curl -fsSL "${url}" -o "${TMP}/imlec.tar.gz" || return 1
  tar -xzf "${TMP}/imlec.tar.gz" -C "${TMP}" || return 1
  mkdir -p "${BIN_DIR}"
  install -Dm755 "${TMP}/imlec" "${BIN_DIR}/imlec"
  [ -f "${TMP}/imlec.service" ] && cp "${TMP}/imlec.service" "${TMP}/unit.service"
  return 0
}

build_from_source() {
  command -v cargo >/dev/null 2>&1 || die "no release binary available and cargo is not installed (pacman -S rustup && rustup default stable)"
  info "building from source"
  git clone --depth 1 "https://github.com/${REPO}.git" "${TMP}/src"
  (cd "${TMP}/src" && cargo build --release --bin imlec)
  mkdir -p "${BIN_DIR}"
  install -Dm755 "${TMP}/src/target/release/imlec" "${BIN_DIR}/imlec"
  cp "${TMP}/src/packaging/systemd/imlec.service" "${TMP}/unit.service"
}

info "installing imlec to ${BIN_DIR}"
if ! install_from_release; then
  warn "no prebuilt release found, falling back to a source build"
  build_from_source
fi

# --- input group -------------------------------------------------------------
# Reading /dev/input is what makes the effect work in every application,
# including on Wayland where global key events are otherwise unavailable.
if id -nG "${USER}" | tr ' ' '\n' | grep -qx input; then
  info "user is already in the 'input' group"
else
  warn "your user is not in the 'input' group; imlec cannot read keystrokes without it"
  if ask "Run 'sudo usermod -aG input ${USER}' now?"; then
    sudo usermod -aG input "${USER}"
    warn "log out and back in for the group change to take effect"
  else
    echo "    run this yourself later:  sudo usermod -aG input ${USER}"
  fi
fi

# --- autostart ---------------------------------------------------------------
if [ -f "${TMP}/unit.service" ] && command -v systemctl >/dev/null 2>&1; then
  mkdir -p "${UNIT_DIR}"
  cp "${TMP}/unit.service" "${UNIT_DIR}/imlec.service"
  systemctl --user daemon-reload || true
  info "installed ${UNIT_DIR}/imlec.service"
  if ask "Enable imlec at login via systemd?"; then
    systemctl --user enable --now imlec.service || warn "could not enable the unit; start it manually with 'imlec'"
  fi
fi

case ":${PATH}:" in
  *":${BIN_DIR}:"*) ;;
  *) warn "${BIN_DIR} is not on your PATH; add it in your shell rc" ;;
esac

cat <<EOF

  imlec installed.

    imlec                       run it
    imlec --print-config-path   where the config lives
    imlec --reset-config        restore the commented defaults

  Hyprland users who prefer not to use systemd can autostart it with:

    exec-once = ${BIN_DIR}/imlec

EOF
