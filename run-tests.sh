#!/usr/bin/env bash
set -euo pipefail

repo_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
cd "$repo_dir"

case "${1:-all}" in
    all)
        cargo fmt --check
        cargo clippy --all-targets --all-features --locked -- -D warnings
        cargo test --all-targets --all-features --locked
        ;;
    unit)
        shift
        cargo test --all-targets --all-features --locked "$@"
        ;;
    *)
        echo "Usage: $0 [all|unit [test-filter]]" >&2
        exit 2
        ;;
esac
