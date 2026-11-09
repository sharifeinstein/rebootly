//! Regenerate `tests/fixtures/*` from `rebootly_core::fixtures`.
//! Run from the repo root:
//! `cargo run -p rebootly-core --example gen_fixtures --features fixtures`

use std::path::Path;

fn main() -> std::io::Result<()> {
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/fixtures/win11-basic");
    rebootly_core::fixtures::build_win11_basic(&root)?;
    println!("wrote {}", root.display());
    Ok(())
}
