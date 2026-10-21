#!/usr/bin/env bash
# Builds the spike APK: the Rust core for arm64 (cargo-ndk), Kotlin bindings from the built
# library (UniFFI library mode), then Gradle. Run from Git Bash at the worktree root.
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
root="$(cd "$here/.." && pwd)"
source "$here/env.sh"
app="$here/android/app/src/main"
cd "$root"
cargo ndk -t arm64-v8a --platform 26 build --release -p venuta-spike-ffi
# Copy only the facade: `cargo ndk -o` also copies the cdylibs iroh and iroh-relay declare
# (libiroh-*.so, libiroh_relay-*.so, 1.3 MB nobody loads).
rm -rf "$app/jniLibs"
mkdir -p "$app/jniLibs/arm64-v8a"
cp target/aarch64-linux-android/release/libvenuta_spike.so "$app/jniLibs/arm64-v8a/"
cargo build --release -p uniffi-bindgen
rm -rf "$app/java-gen"
./target/release/uniffi-bindgen generate --library target/aarch64-linux-android/release/libvenuta_spike.so \
  --language kotlin --out-dir "$app/java-gen" --no-format
cd "$here/android"
./gradlew --no-daemon assembleRelease
