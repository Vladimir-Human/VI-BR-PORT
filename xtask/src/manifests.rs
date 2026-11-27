//! Every workspace crate takes its settings from the workspace: the lints of
//! the root `Cargo.toml` (`[lints] workspace = true`, so `unsafe_op_in_unsafe_fn`
//! and the clippy settings reach it), the package fields (`license` and the
//! rest), and each dependency (`name.workspace = true`, so versions are kept
//! in one place and a new dependency shows in the root manifest).
//!
//! The manifests are read as text, line by line, which is enough for the form
//! they are written in.

use std::path::Path;

use anyhow::{Context, Result};

/// Package fields every member inherits.
const INHERITED: &[&str] = &["version", "edition", "rust-version", "license", "publish"];

pub fn run(root: &Path) -> Result<Vec<String>> {
    let workspace = std::fs::read_to_string(root.join("Cargo.toml")).context("Cargo.toml")?;
    let mut problems = Vec::new();
    for member in members(root, &workspace)? {
        let path = format!("{member}/Cargo.toml");
        let text = std::fs::read_to_string(root.join(&path)).with_context(|| path.clone())?;
        problems.extend(check(&path, &text));
    }
    Ok(problems)
}

/// The member folders the root manifest names, `dir/*` expanded to the
/// folders below `dir` that hold a `Cargo.toml`.
fn members(root: &Path, workspace: &str) -> Result<Vec<String>> {
    let line = workspace
        .lines()
        .find(|l| l.trim_start().starts_with("members"))
        .context("no `members` line in Cargo.toml")?;
    let mut out = Vec::new();
    for entry in line.split('"').skip(1).step_by(2) {
        match entry.strip_suffix("/*") {
            Some(dir) => {
                let mut found: Vec<String> = std::fs::read_dir(root.join(dir))
                    .with_context(|| dir.to_owned())?
                    .filter_map(Result::ok)
                    .filter(|e| e.path().join("Cargo.toml").is_file())
                    .map(|e| format!("{dir}/{}", e.file_name().to_string_lossy()))
                    .collect();
                found.sort();
                out.extend(found);
            }
            None => out.push(entry.to_owned()),
        }
    }
    Ok(out)
}

/// The key of a `key = value` line (`a.b = ...` gives `a.b`).
fn key(line: &str) -> Option<&str> {
    let (k, _) = line.split_once('=')?;
    let k = k.trim();
    (!k.is_empty() && !k.starts_with('#')).then_some(k)
}

/// All problems with one member manifest. Pure, for testing.
pub fn check(path: &str, text: &str) -> Vec<String> {
    let mut problems = Vec::new();
    let mut section = String::new();
    let mut lints_inherited = false;
    let mut inherited: Vec<&str> = Vec::new();
    // A `[dependencies.name]` table: its name, line, and whether it inherits.
    let mut table: Option<(String, usize, bool)> = None;
    let mut depth = 0i32;
    let close_table = |table: &mut Option<(String, usize, bool)>, problems: &mut Vec<String>| {
        if let Some((name, line, false)) = table.take() {
            problems.push(format!(
                "{path}:{line}: dependency `{name}` does not come from the workspace (`workspace = true`)"
            ));
        }
    };
    for (n, raw) in text.lines().enumerate() {
        let line = raw.split(" #").next().unwrap_or(raw).trim();
        let continued = depth > 0;
        depth += line.matches(['[', '{']).count() as i32 - line.matches([']', '}']).count() as i32;
        if continued || line.is_empty() || line.starts_with('#') {
            continue;
        }
        if line.starts_with('[') {
            depth = 0;
            close_table(&mut table, &mut problems);
            section = line.trim_matches(['[', ']']).trim().to_owned();
            if let Some((_, name)) = section.rsplit_once("dependencies.") {
                table = Some((name.to_owned(), n + 1, false));
            }
            continue;
        }
        let Some(k) = key(line) else { continue };
        let value = line.split_once('=').map_or("", |(_, v)| v);
        let compact: String = line.split_whitespace().collect();
        match section.as_str() {
            "lints" if k == "workspace" && value.trim() == "true" => lints_inherited = true,
            "package" => {
                if let Some(field) = k.strip_suffix(".workspace")
                    && value.trim() == "true"
                {
                    inherited.push(field);
                } else if compact.contains("{workspace=true") {
                    inherited.push(k);
                }
            }
            s if s.ends_with("dependencies") => {
                let from_workspace = k.ends_with(".workspace") && value.trim() == "true"
                    || compact.contains("workspace=true");
                if !from_workspace {
                    problems.push(format!(
                        "{path}:{}: dependency `{k}` does not come from the workspace (`{k}.workspace = true`)",
                        n + 1
                    ));
                }
            }
            _ => {
                if let Some((_, _, from)) = table.as_mut()
                    && k == "workspace"
                    && value.trim() == "true"
                {
                    *from = true;
                }
            }
        }
    }
    close_table(&mut table, &mut problems);
    if !lints_inherited {
        problems.push(format!(
            "{path}: no `[lints] workspace = true`: the workspace lints do not reach this crate"
        ));
    }
    for field in INHERITED {
        if !inherited.contains(field) {
            problems.push(format!(
                "{path}: `{field}` is not taken from the workspace (`{field}.workspace = true`)"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::check;

    /// Enforces the rule wherever tests run.
    #[test]
    fn workspace_crates_inherit_lints_and_settings() {
        let problems = super::run(&crate::workspace_root()).unwrap();
        assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
    }

    const GOOD: &str = "[package]\nname = \"a\"\nversion.workspace = true\nedition.workspace = true\n\
                        rust-version.workspace = true\nlicense = { workspace = true }\npublish.workspace = true\n\n\
                        [dependencies]\nanyhow.workspace = true\nwindows = { workspace = true, features = [\n    \"A\",\n] }\n\n\
                        [target.'cfg(windows)'.dependencies]\nblake3.workspace = true # hashing\n\n\
                        [dev-dependencies.tempfile]\nworkspace = true\n\n[lints]\nworkspace = true\n";

    #[test]
    fn a_manifest_that_inherits_everything_passes() {
        let p = check("crates/a/Cargo.toml", GOOD);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn missing_lints_license_or_workspace_dependencies_fail() {
        let bad = GOOD
            .replace("[lints]\nworkspace = true\n", "")
            .replace("license = { workspace = true }", "license = \"MIT\"")
            .replace("anyhow.workspace = true", "anyhow = \"1\"")
            .replace(
                "[dev-dependencies.tempfile]\nworkspace = true",
                "[dev-dependencies.tempfile]\nversion = \"3\"",
            );
        let p = check("crates/a/Cargo.toml", &bad);
        assert_eq!(p.len(), 4, "{p:?}");
        assert!(p.iter().any(|m| m.contains("`anyhow`")));
        assert!(p.iter().any(|m| m.contains("`tempfile`")));
        assert!(p.iter().any(|m| m.contains("[lints]")));
        assert!(p.iter().any(|m| m.contains("`license`")));
    }
}
