#!/bin/sh
# Installs SidFind into the current user's home (no root needed): the menu entry and the icons.
# Run it from the folder the release archive was unpacked to:
#
#   ./install.sh             install
#   ./install.sh --uninstall remove again
#
# The menu entry points at sidfind-gui in this folder, so do not move the folder afterwards
# (or run the script again from its new place).

set -eu

here=$(cd "$(dirname "$0")" && pwd)
share="${XDG_DATA_HOME:-$HOME/.local/share}"
apps="$share/applications/sidfind-gui.desktop"
icon_root="$share/icons/hicolor"

uninstall() {
    rm -f "$apps"
    for png in "$here"/icons/sidfind-*.png; do
        size=$(basename "$png" .png)
        size=${size#sidfind-}
        rm -f "$icon_root/${size}x${size}/apps/sidfind.png"
    done
    refresh
    echo "SidFind menu entry and icons removed."
}

refresh() {
    command -v gtk-update-icon-cache > /dev/null 2>&1 && gtk-update-icon-cache -q -t "$icon_root" 2> /dev/null || true
    command -v update-desktop-database > /dev/null 2>&1 && update-desktop-database "$share/applications" 2> /dev/null || true
}

if [ "${1:-}" = "--uninstall" ]; then
    uninstall
    exit 0
fi

if [ ! -x "$here/sidfind-gui" ]; then
    echo "sidfind-gui not found next to this script ($here)." >&2
    exit 1
fi

# icons: icons/sidfind-<size>.png goes to hicolor/<size>x<size>/apps/sidfind.png
for png in "$here"/icons/sidfind-*.png; do
    size=$(basename "$png" .png)
    size=${size#sidfind-}
    mkdir -p "$icon_root/${size}x${size}/apps"
    cp "$png" "$icon_root/${size}x${size}/apps/sidfind.png"
done

# menu entry, with the real path of the program in Exec (quoted, so spaces work). The path is
# escaped twice: once for the desktop file syntax, once for the sed replacement text.
mkdir -p "$(dirname "$apps")"
exec_path=$(printf '%s' "$here/sidfind-gui" | sed 's/\\/\\\\/g; s/"/\\"/g')
exec_repl=$(printf '%s' "$exec_path" | sed 's/[&|\\]/\\&/g')
sed "s|^Exec=.*|Exec=\"$exec_repl\"|" "$here/sidfind-gui.desktop" > "$apps"

refresh
echo "SidFind installed. Look for it in your application menu."
echo "To remove it again, run: $here/install.sh --uninstall"
