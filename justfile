fmt:
    @cargo fmt
    @taplo fmt

verify:
    @cargo fmt --all -- --check
    @cargo clippy --all-targets --locked
    @cargo test
