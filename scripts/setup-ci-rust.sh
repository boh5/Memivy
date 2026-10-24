#!/usr/bin/env bash
set -euo pipefail

cargo_bin="${CARGO_HOME:-$HOME/.cargo}/bin"
if [[ ! -x "$cargo_bin/rustup" || ! -x "$cargo_bin/cargo" || ! -x "$cargo_bin/rustc" ]]; then
  installer="$RUNNER_TEMP/memivy-rustup-init.sh"
  curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs -o "$installer"
  sh "$installer" -y --profile minimal --default-toolchain none --no-modify-path
fi

export PATH="$cargo_bin:$PATH"
printf '%s\n' "$cargo_bin" >> "$GITHUB_PATH"
rustup show
rustc --version
cargo --version
