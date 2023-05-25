#!/usr/bin/env bash
BIN="ompl"
FEATURES=""

LINUX="./target/release/${BIN}"
WIN="./target/release/${BIN}.exe"

cargo build --release $FEATURES

mkdir ./bin 2>/dev/null

if [ -f "$LINUX" ]; then
    cp "$LINUX" "./bin/${BIN}"
    cargo build --release --no-default-features --features=media-controls,tui,clipboard,backend-rodio --target x86_64-pc-windows-gnu
    cp "./target/x86_64-pc-windows-gnu/release/${BIN}.exe" "./bin/${BIN}.exe"
fi

if [ -f "$WIN" ]; then
    cp "$WIN" "./bin/${BIN}.exe"
    cargo build --release $FEATURES --target x86_64-unknown-linux-gnu
    cp "./target/x86_64-unknown-linux-gnu/release/${BIN}" "./bin/${BIN}"
fi
