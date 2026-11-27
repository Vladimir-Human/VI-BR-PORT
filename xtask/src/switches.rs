//! Every `VICTORY_*` switch the shader translator reads must be part of the
//! shader cache's key (`crates/victory-gpu/src/shader_cache.rs`, `build()`).
//! A switch that changes the emitted code but is not in the key serves a run
//! the translations an earlier run cached with another value: the cache is
//! stale and nothing says so.
//!
//! The check reads the sources as text (`rust_text`): the switches are the
//! string literals `"VICTORY_..."` in the translator's code outside test-only
//! items, and the key's switches are the same literals in the body of the
//! key's function. A switch the key takes through a call into the translator
//! (`loop_limit()` and the like) is listed in [`THROUGH_CALLS`]; one that
//! does not change the emitted code is listed in [`DIAGNOSTICS`] with why.

use std::collections::{BTreeMap, BTreeSet};
use std::path::Path;

use anyhow::{Context, Result};

use crate::rust_text::{self, Kind, TestOnly};

/// The translator's sources, relative to the workspace root.
pub const TRANSLATOR: &str = "crates/victory-shader/src";
/// The file that holds the shader cache's key.
pub const CACHE: &str = "crates/victory-gpu/src/shader_cache.rs";
/// The function whose body is the key.
pub const KEY_FN: &str = "build";

/// Switches the key takes through a call into the translator: (switch, the
/// call as the key's function writes it, the translator function that reads
/// the switch).
pub const THROUGH_CALLS: &[(&str, &str, &str)] = &[
    (
        "VICTORY_SHADER_LOOP_LIMIT",
        "victory_shader::emit::loop_limit()",
        "loop_limit",
    ),
    (
        "VICTORY_GROUP_SYNC",
        "victory_shader::emit::group_sync()",
        "group_sync",
    ),
    (
        "VICTORY_SHADER_OPT",
        "victory_shader::emit::shader_opt()",
        "shader_opt",
    ),
    // `host_settings()` holds `host_rdna()`, which is `rdna_float()` on an
    // RDNA host.
    (
        "VICTORY_RDNA_FLOAT",
        "victory_shader::emit::host_settings()",
        "rdna_float",
    ),
];

/// Switches the translator reads that do not change what it emits, with the
/// reason. Keep it short: a switch belongs here only if it really changes
/// nothing a cached translation holds.
pub const DIAGNOSTICS: &[(&str, &str)] = &[
    (
        "VICTORY_WAVE_WHY",
        "stderr lines (WAVE64-WHY, WAVE64-RETRY, WAVE64-WIDE, WAVE64-OWN-BITS) saying why a wave64 program took its form",
    ),
    (
        "VICTORY_DESC_WHY",
        "a stderr line (DESC-WHY) per descriptor load the lowering could not place",
    ),
    (
        "VICTORY_SHADER_CENSUS",
        "counts what a written module holds (bitcode::census); its tags are read by the census only",
    ),
    (
        "VICTORY_WARP",
        "emit::host, the offline tools' device: picks the WARP adapter that pipelines are made on, not the code",
    ),
    (
        "VICTORY_D3D12_DEBUG",
        "emit::host: turns on the D3D12 debug layer for the offline tools' device",
    ),
    (
        "VICTORY_DXIL",
        "where dxil.dll is found (validation and signing, whose digest is of the content)",
    ),
];

/// One source file, its path relative to the workspace root.
pub struct Source {
    pub path: String,
    pub text: String,
}

/// Reads the translator's sources and the cache file, and checks them.
pub fn run(root: &Path) -> Result<Vec<String>> {
    let mut translator = Vec::new();
    for entry in walkdir::WalkDir::new(root.join(TRANSLATOR)).sort_by_file_name() {
        let entry = entry?;
        if entry.file_type().is_file() && entry.path().extension().is_some_and(|e| e == "rs") {
            let rel = entry
                .path()
                .strip_prefix(root)?
                .to_string_lossy()
                .replace('\\', "/");
            let text = std::fs::read_to_string(entry.path()).with_context(|| rel.clone())?;
            translator.push(Source { path: rel, text });
        }
    }
    let cache = std::fs::read_to_string(root.join(CACHE)).context(CACHE)?;
    Ok(check(&translator, &cache, THROUGH_CALLS, DIAGNOSTICS))
}

/// Files of `sources` that only test builds compile: those declared
/// `#[cfg(test)] mod name;` and everything below them.
fn test_only_files(sources: &[Source], parsed: &[TestOnly]) -> BTreeSet<String> {
    let mut dirs = Vec::new();
    for (s, t) in sources.iter().zip(parsed) {
        let (parent, stem) = match s.path.rsplit_once('/') {
            Some((p, f)) => (p, f.trim_end_matches(".rs")),
            None => ("", s.path.trim_end_matches(".rs")),
        };
        let dir = if matches!(stem, "lib" | "main" | "mod") {
            parent.to_owned()
        } else {
            format!("{parent}/{stem}")
        };
        for m in &t.modules {
            dirs.push(format!("{dir}/{m}"));
        }
    }
    sources
        .iter()
        .filter(|s| {
            dirs.iter()
                .any(|d| s.path == format!("{d}.rs") || s.path.starts_with(&format!("{d}/")))
        })
        .map(|s| s.path.clone())
        .collect()
}

/// The byte range of the body of `fn name` in `code` (comments and literals
/// blanked), braces included.
fn fn_body(code: &str, name: &str) -> Option<std::ops::Range<usize>> {
    let needle = format!("fn {name}(");
    let at = code.find(&needle)?;
    let open = at + code[at..].find('{')?;
    let mut depth = 0usize;
    for (j, c) in code.bytes().enumerate().skip(open) {
        match c {
            b'{' => depth += 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(open..j + 1);
                }
            }
            _ => {}
        }
    }
    None
}

/// The `"VICTORY_..."` literals of `src` that name a switch read at run time,
/// with their offsets: not the names of build-time variables (`env!("...")`,
/// which build scripts set).
fn switch_literals(src: &str) -> Vec<(&str, usize)> {
    let pieces = rust_text::pieces(src);
    let mut out = Vec::new();
    for (i, p) in pieces.iter().enumerate() {
        let Some(name) = p.str_value().and_then(switch_name) else {
            continue;
        };
        let build_time = i > 0
            && pieces[i - 1].kind == Kind::Code
            && pieces[i - 1].text.trim_end().ends_with("env!(");
        if !build_time {
            out.push((name, p.start));
        }
    }
    out
}

/// The switch a string literal names, if the whole literal is one name.
fn switch_name(value: &str) -> Option<&str> {
    let name = value.strip_prefix("VICTORY_")?;
    (!name.is_empty()
        && name
            .bytes()
            .all(|b| b.is_ascii_uppercase() || b.is_ascii_digit() || b == b'_'))
    .then_some(value)
}

/// All problems. Pure, for testing.
pub fn check(
    translator: &[Source],
    cache: &str,
    through: &[(&str, &str, &str)],
    diagnostics: &[(&str, &str)],
) -> Vec<String> {
    let mut problems = Vec::new();
    let parsed: Vec<TestOnly> = translator
        .iter()
        .map(|s| rust_text::test_only(&s.text))
        .collect();
    let skip = test_only_files(translator, &parsed);

    // What the translator reads, with where it is first read.
    let mut read: BTreeMap<String, String> = BTreeMap::new();
    for (s, t) in translator.iter().zip(&parsed) {
        if skip.contains(&s.path) {
            continue;
        }
        for (name, at) in switch_literals(&s.text) {
            if !t.contains(at) {
                read.entry(name.to_owned())
                    .or_insert_with(|| format!("{}:{}", s.path, rust_text::line_of(&s.text, at)));
            }
        }
    }

    // What the key names.
    let code = rust_text::code_only(cache);
    let Some(body) = fn_body(&code, KEY_FN) else {
        problems.push(format!(
            "{CACHE}: no `fn {KEY_FN}(` found; if the cache key moved, update xtask/src/switches.rs"
        ));
        return problems;
    };
    let keyed: BTreeSet<String> = switch_literals(cache)
        .into_iter()
        .filter(|(_, at)| body.contains(at))
        .map(|(name, _)| name.to_owned())
        .collect();

    // Switches the key takes through a call: the call must be in the key and
    // the translator function must read the switch.
    let mut through_ok = BTreeSet::new();
    for &(name, call, function) in through {
        if !code[body.clone()].contains(call) {
            problems.push(format!(
                "{name}: the key ({CACHE}, fn {KEY_FN}) no longer calls `{call}`"
            ));
            continue;
        }
        let reads = translator.iter().any(|s| {
            let code = rust_text::code_only(&s.text);
            fn_body(&code, function).is_some_and(|b| {
                rust_text::pieces(&s.text)
                    .iter()
                    .any(|p| b.contains(&p.start) && p.str_value() == Some(name))
            })
        });
        if !reads {
            problems.push(format!(
                "{name}: `fn {function}` in {TRANSLATOR} no longer reads it; update THROUGH_CALLS in xtask/src/switches.rs"
            ));
            continue;
        }
        through_ok.insert(name);
    }

    for (name, at) in &read {
        let diagnostic = diagnostics.iter().any(|(d, _)| d == name);
        if !keyed.contains(name) && !through_ok.contains(name.as_str()) && !diagnostic {
            problems.push(format!(
                "{name} ({at}) is read by the translator but is not in the shader cache key: \
                 add it to the list in {CACHE} (fn {KEY_FN}), or, if it changes nothing the \
                 translation holds, to DIAGNOSTICS in xtask/src/switches.rs with the reason"
            ));
        }
    }
    // A keyed switch the cache file reads itself, outside the key's function
    // (`VICTORY_WAVE_WIDE_FOR`, which picks per program how the translator is
    // run), is not stale: what it changes is the translation all the same.
    let cache_test = rust_text::test_only(cache);
    let read_by_cache: BTreeSet<&str> = switch_literals(cache)
        .into_iter()
        .filter(|(_, at)| !body.contains(at) && !cache_test.contains(*at))
        .map(|(name, _)| name)
        .collect();
    for name in &keyed {
        if !read.contains_key(name) && !read_by_cache.contains(name.as_str()) {
            problems.push(format!(
                "{name} is in the shader cache key ({CACHE}) but the translator does not read it"
            ));
        }
    }
    for (name, _) in diagnostics {
        if !read.contains_key(*name) {
            problems.push(format!(
                "{name} is on DIAGNOSTICS in xtask/src/switches.rs but the translator does not read it"
            ));
        }
        if keyed.contains(*name) {
            problems.push(format!(
                "{name} is both in the shader cache key and on DIAGNOSTICS in xtask/src/switches.rs"
            ));
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use super::{Source, check};

    /// Enforces the rule wherever tests run.
    #[test]
    fn every_translator_switch_is_in_the_shader_cache_key() {
        let problems = super::run(&crate::workspace_root()).unwrap();
        assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
    }

    fn src(path: &str, text: &str) -> Source {
        Source {
            path: path.into(),
            text: text.into(),
        }
    }

    const CACHE: &str = "fn build() -> [u8; 32] {\n    tracing::warn!(\"VICTORY_A: a message\");\n    \
                         let _ = env!(\"VICTORY_BUILT\");\n    \
                         for name in [\"VICTORY_A\", \"VICTORY_B\"] {}\n    victory_shader::emit::limit();\n}\n\
                         fn other() { let _ = \"VICTORY_OTHER\"; }\n";

    #[test]
    fn passes_when_every_switch_is_keyed() {
        let lib = src(
            "crates/victory-shader/src/lib.rs",
            "fn a() { var(\"VICTORY_A\"); }\nfn b() { var(\"VICTORY_B\"); env!( \"VICTORY_HASH\"); }\n\
             pub fn limit() -> u32 { var(\"VICTORY_L\") }\nfn d() { var(\"VICTORY_D\"); }\n",
        );
        let through = [("VICTORY_L", "victory_shader::emit::limit()", "limit")];
        let p = check(&[lib], CACHE, &through, &[("VICTORY_D", "prints")]);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn a_switch_missing_from_the_key_fails() {
        let lib = src(
            "crates/victory-shader/src/lib.rs",
            "fn a() { var(\"VICTORY_A\"); var(\"VICTORY_B\"); var(\"VICTORY_NEW\"); }\n",
        );
        let p = check(&[lib], CACHE, &[], &[]);
        assert_eq!(p.len(), 1, "{p:?}");
        assert!(
            p[0].starts_with("VICTORY_NEW (crates/victory-shader/src/lib.rs:1)"),
            "{p:?}"
        );
    }

    #[test]
    fn switches_in_test_code_and_comments_do_not_count() {
        let lib = src(
            "crates/victory-shader/src/lib.rs",
            "// \"VICTORY_COMMENT\"\nfn a() { var(\"VICTORY_A\"); var(\"VICTORY_B\"); }\n\
             #[cfg(test)]\nmod diff_tests;\n#[cfg(test)]\nmod tests { fn t() { var(\"VICTORY_T\"); } }\n",
        );
        let far = src(
            "crates/victory-shader/src/diff_tests.rs",
            "fn t() { var(\"VICTORY_FAR\"); }\n",
        );
        let p = check(&[lib, far], CACHE, &[], &[]);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn stale_entries_fail() {
        let lib = src(
            "crates/victory-shader/src/lib.rs",
            "fn a() { var(\"VICTORY_A\"); }\n",
        );
        let through = [("VICTORY_L", "victory_shader::emit::limit()", "limit")];
        let p = check(&[lib], CACHE, &through, &[("VICTORY_GONE", "prints")]);
        // VICTORY_B keyed but unread, VICTORY_L's function gone, VICTORY_GONE unread.
        assert_eq!(p.len(), 3, "{p:?}");
    }

    #[test]
    fn a_keyed_switch_the_cache_reads_itself_is_not_stale() {
        let lib = src(
            "crates/victory-shader/src/lib.rs",
            "fn a() { var(\"VICTORY_A\"); }\n",
        );
        let cache = format!("{CACHE}fn pick() {{ var(\"VICTORY_B\"); }}\n");
        let p = check(&[lib], &cache, &[], &[]);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn a_missing_key_function_fails() {
        let p = check(&[], "fn renamed() {}", &[], &[]);
        assert_eq!(p.len(), 1, "{p:?}");
    }
}
