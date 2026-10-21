#!/usr/bin/env bash
# Builds OxyMail in release mode and installs it for the current user:
# ~/.local/bin/oxymail, a menu entry, and icons. No root needed. Run again to update.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
target="${CARGO_TARGET_DIR:-$HOME/.cache/oxymail-target}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"

cd "$repo/app"
[ -d node_modules ] || npm install
# Built without the Tauri CLI, which needs file watches that a busy desktop can run out of.
# The custom-protocol feature is what `tauri build` adds: it embeds the built frontend.
npm run build
(cd "$repo" && CARGO_TARGET_DIR="$target" cargo build --release -p oxymail-app --features tauri/custom-protocol)

install -Dm755 "$target/release/oxymail" "$HOME/.local/bin/oxymail"

icons="$repo/app/src-tauri/icons"
for size in 32 64 128; do
  install -Dm644 "$icons/${size}x${size}.png" "$data/icons/hicolor/${size}x${size}/apps/oxymail.png"
done
install -Dm644 "$icons/128x128@2x.png" "$data/icons/hicolor/256x256/apps/oxymail.png"
install -Dm644 "$icons/icon.png" "$data/icons/hicolor/512x512/apps/oxymail.png"
install -Dm644 "$icons/icon.svg" "$data/icons/hicolor/scalable/apps/oxymail.svg"

install -Dm644 /dev/stdin "$data/applications/oxymail.desktop" <<DESKTOP
[Desktop Entry]
Type=Application
Name=OxyMail
GenericName=Email
Comment=Email with room to breathe
Exec=$HOME/.local/bin/oxymail %u
Icon=oxymail
Terminal=false
Categories=Network;Email;Office;
Keywords=mail;email;inbox;
StartupWMClass=oxymail
StartupNotify=true
MimeType=x-scheme-handler/mailto;
DESKTOP

update-desktop-database "$data/applications" 2>/dev/null || true
gtk-update-icon-cache -q -t "$data/icons/hicolor" 2>/dev/null || true
echo "Installed. Launch OxyMail from the app menu, or run: oxymail"
