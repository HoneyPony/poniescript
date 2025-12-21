# Runs the main integration test suite. Apparently, cargo does not actually
# have a way to build the Rust runtime before running the integration tests,
# so we just have to do it manually. :(
#
# See https://github.com/rust-lang/cargo/issues/8311

cargo build -p poniescript-rt
(cd poniescript && cargo test)