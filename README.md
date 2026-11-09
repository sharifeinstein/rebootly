# Rebootly

Portable PC migration and backup tool for Windows (live Windows, WinPE, Linux recovery).
The specification is [docs/SPEC.md](docs/SPEC.md); status is in [docs/PROGRESS.md](docs/PROGRESS.md).

```sh
cargo test
cargo run -p rebootly-cli -- inspect --windows-root tests/fixtures/win11-basic
cargo run -p rebootly-cli -- inspect --scan /mnt --json
```

Regenerate test fixtures: `cargo run -p rebootly-core --example gen_fixtures --features fixtures`.
