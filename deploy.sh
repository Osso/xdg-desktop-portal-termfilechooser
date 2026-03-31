#!/bin/bash
set -euo pipefail

cd "$(dirname "$0")"

pkgname=xdg-desktop-portal-termfilechooser

if pacman -Q "$pkgname" >/dev/null 2>&1; then
    authsudo pacman -R --noconfirm "$pkgname"
fi

authsudo cargo install --path . --locked --force --root /usr
authsudo install -Dm644 termfilechooser.portal /usr/share/xdg-desktop-portal/portals/termfilechooser.portal
authsudo install -Dm644 \
    org.freedesktop.impl.portal.desktop.termfilechooser.service \
    /usr/share/dbus-1/services/org.freedesktop.impl.portal.desktop.termfilechooser.service

# Kill the running instance — D-Bus will auto-restart it on next use
pkill -x xdg-desktop-portal-termfilechooser 2>/dev/null || true

echo "Deployed. Service will restart on next file dialog."
