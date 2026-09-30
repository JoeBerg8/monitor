#!/bin/sh
set -eu

if [ "$(uname -s)" != "Linux" ]; then
  echo "monitor Linux installer must run on Linux" >&2
  exit 1
fi

repo_dir=$(CDPATH= cd -- "$(dirname -- "$0")/.." && pwd)
cargo build --release --locked --manifest-path "$repo_dir/linux/Cargo.toml"
"$repo_dir/linux/target/release/monitor" install "$@"

echo "Run '$HOME/.local/bin/monitor doctor' and then '$HOME/.local/bin/monitor start'."
