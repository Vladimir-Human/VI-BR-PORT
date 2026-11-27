//! `cargo xtask rules`: the checks of this fork's own rules, which
//! `cargo test -p xtask` also runs one by one (each module's tests).

use std::path::Path;

use anyhow::{Result, bail};

/// Each check: its name and what it finds wrong.
type Check = fn(&Path) -> Result<Vec<String>>;

const CHECKS: &[(&str, Check)] = &[
    ("shader cache switches", crate::switches::run),
    ("English sources", crate::english::run),
    ("documentation links", crate::doc_links::run),
    ("crate manifests", crate::manifests::run),
];

pub fn run(root: &Path) -> Result<()> {
    let mut failed = 0;
    for (name, check) in CHECKS {
        let problems = check(root)?;
        if problems.is_empty() {
            println!("{name}: ok");
        } else {
            failed += problems.len();
            for p in &problems {
                eprintln!("{name}: {p}");
            }
        }
    }
    if failed > 0 {
        bail!("{failed} problem(s); see docs/ci.md");
    }
    Ok(())
}
