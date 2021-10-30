cargo build --release --target x86_64-unknown-linux-gnu
cargo build --release --target x86_64-pc-windows-gnu
mkdir ./bin 2> /dev/null
cp ./target/x86_64-unknown-linux-gnu/release/ompl ./bin/
cp ./target/x86_64-pc-windows-gnu/release/ompl.exe ./bin/
strip ./bin/*
