#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
pkgname=xdg-desktop-portal-termfilechooser

if [[ -n $(git -C "$repo_dir" status --porcelain) ]]; then
    echo "Refusing to package an uncommitted checkout." >&2
    exit 1
fi

authsudo arch install "$repo_dir/packaging"
"/usr/bin/$pkgname" --configure-portal

pkill -f "^/usr/bin/$pkgname( |$)" 2>/dev/null || true
systemctl --user restart xdg-desktop-portal.service

echo "Deployed package and selected termfilechooser for FileChooser."
