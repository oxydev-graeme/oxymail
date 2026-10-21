#!/usr/bin/env bash
# Builds OxyMail from source and installs it for the current user (for development; normal
# installs use the released AppImage or .deb, which update themselves):
# ~/.local/bin/oxymail, a menu entry, and icons. No root needed. Run again to update.
set -euo pipefail

repo="$(cd "$(dirname "$0")/.." && pwd)"
target="${CARGO_TARGET_DIR:-$HOME/.cache/oxymail-target}"
data="${XDG_DATA_HOME:-$HOME/.local/share}"

# Optional sign-in client IDs compiled into this copy (kept out of the public repo):
#   OXYMAIL_GOOGLE_CLIENT_ID=…  OXYMAIL_GOOGLE_CLIENT_SECRET=…  OXYMAIL_MICROSOFT_CLIENT_ID=…
oauth_env="${XDG_CONFIG_HOME:-$HOME/.config}/oxymail/oauth.env"
if [ -f "$oauth_env" ]; then
  set -a; . "$oauth_env"; set +a
  echo "Using sign-in clients from $oauth_env"
fi

cd "$repo/app"
[ -d node_modules ] || npm install
# Built without the Tauri CLI, which needs file watches that a busy desktop can run out of.
# The custom-protocol feature is what `tauri build` adds: it embeds the built frontend.
npm run build
(cd "$repo" && CARGO_TARGET_DIR="$target" cargo build --release -p oxymail-app --features tauri/custom-protocol)

# If the released AppImage is installed (it updates itself), keep it as the main copy and put
# this development build alongside as `oxymail-dev`.
if [ -x "$HOME/.local/bin/OxyMail.AppImage" ]; then
  install -Dm755 "$target/release/oxymail" "$HOME/.local/bin/oxymail-dev"
  echo "Installed the development build as oxymail-dev; the menu entry still opens the released AppImage."
  exit 0
fi
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
