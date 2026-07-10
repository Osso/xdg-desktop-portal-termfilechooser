#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
pkgname=xdg-desktop-portal-termfilechooser

if [[ -n $(git -C "$repo_dir" status --porcelain) ]]; then
    echo "Refusing to package an uncommitted checkout." >&2
    exit 1
fi

git -C "$repo_dir" fetch origin master
local_head=$(git -C "$repo_dir" rev-parse HEAD)
remote_head=$(git -C "$repo_dir" rev-parse origin/master)
if [[ "$local_head" != "$remote_head" ]]; then
    echo "Refusing to package: local HEAD does not match origin/master." >&2
    exit 1
fi

authsudo arch install "$repo_dir/packaging"
"/usr/bin/$pkgname" --configure-portal

pkill -f "^/usr/bin/$pkgname( |$)" 2>/dev/null || true
systemctl --user restart xdg-desktop-portal.service

echo "Deployed package and selected termfilechooser for FileChooser."
