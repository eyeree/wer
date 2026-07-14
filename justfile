default: test-all

build-all: build-windows build-linux build-web

build-windows:
  cargo xwin build --release --bin wer --target x86_64-pc-windows-msvc

build-web:
  cargo run --bin web-build

build-linux:
  echo "bar"

run-linux:
  cargo run --release --bin wer

run-windows: build-windows
  target/x86_64-pc-windows-msvc/release/wer.exe

run-web: build-web
  cargo run --bin web-serve

test-all: test-linux test-web

test-web:
  cargo run --bin web-signoff

test-linux:
  cargo test --workspace

