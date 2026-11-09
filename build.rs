//! Records the Claude versions that have a trace fixture
//! (`tests/fixtures/claude-teams/<version key>.jsonl`, PRD-261108-0447-01 P10.5), so the
//! Launcher can warn about a version this build was not contract-tested against.

use std::path::Path;

fn main() {
    let dir = Path::new("tests/fixtures/claude-teams");
    println!("cargo:rerun-if-changed={}", dir.display());
    let mut keys: Vec<String> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .flatten()
        .filter_map(|entry| {
            let name = entry.file_name().into_string().ok()?;
            name.strip_suffix(".jsonl").map(str::to_owned)
        })
        .collect();
    keys.sort();
    let list = keys
        .iter()
        .map(|key| format!("{key:?}"))
        .collect::<Vec<_>>()
        .join(", ");
    let out = Path::new(&std::env::var("OUT_DIR").unwrap()).join("contract_tested.rs");
    std::fs::write(out, format!("[{list}]")).unwrap();
}
