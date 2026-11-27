//! The sources are written in English: comments, doc comments, string
//! literals and names in `crates/`, `apps/` and `xtask/` hold no letters
//! outside ASCII, which is how text in another language (Portuguese, most
//! often) shows. Symbols are fine (arrows, dashes, the multiplication sign),
//! and so are Greek letters and the micro sign, which English text uses for
//! mathematics and units.
//!
//! A string whose bytes matter (a test of text handling) writes its letters
//! as `\u{...}` escapes; a place that cannot goes on [`ALLOWED`] with why.

use std::path::Path;

use anyhow::Result;

use crate::rust_text::{self, Kind};

/// The folders whose sources are checked.
pub const FOLDERS: &[&str] = &["crates/", "apps/", "xtask/"];

/// Text files besides Rust that hold code or comments we wrote.
const TEXT_EXTENSIONS: &[&str] = &["hlsl", "hlsli", "toml", "ps1", "cmd", "sh"];

/// (path, the word with the letter, why it stays). Keep it empty if at all
/// possible.
pub const ALLOWED: &[(&str, &str, &str)] = &[];

/// Whether `c` is a letter this check turns down.
fn foreign_letter(c: char) -> bool {
    c.is_alphabetic() && !c.is_ascii() && !matches!(c, '\u{b5}' | '\u{370}'..='\u{3ff}')
}

pub fn run(root: &Path) -> Result<Vec<String>> {
    let mut problems = Vec::new();
    for path in crate::policy::list_files(root, false)? {
        if !FOLDERS.iter().any(|f| path.starts_with(f)) {
            continue;
        }
        let ext = path.rsplit_once('.').map(|(_, e)| e);
        let rust = ext == Some("rs");
        if !rust && !ext.is_some_and(|e| TEXT_EXTENSIONS.contains(&e)) {
            continue;
        }
        let Ok(text) = std::fs::read_to_string(root.join(&path)) else {
            continue; // not UTF-8, or gone: the asset policy speaks for those
        };
        problems.extend(check(&path, &text, rust, ALLOWED));
    }
    Ok(problems)
}

/// All problems with one file. Pure, for testing.
pub fn check(path: &str, text: &str, rust: bool, allowed: &[(&str, &str, &str)]) -> Vec<String> {
    let mut problems = Vec::new();
    let pieces = if rust {
        rust_text::pieces(text)
    } else {
        vec![rust_text::Piece {
            kind: Kind::Comment,
            text,
            start: 0,
        }]
    };
    for p in pieces {
        let mut from = 0;
        while let Some(n) = p.text[from..].find(foreign_letter) {
            let at = from + n;
            let start = p.text[..at]
                .rfind(|c: char| !c.is_alphanumeric())
                .map_or(0, |i| {
                    i + p.text[i..].chars().next().map_or(1, char::len_utf8)
                });
            let end = p.text[at..]
                .find(|c: char| !c.is_alphanumeric())
                .map_or(p.text.len(), |i| at + i);
            let word = &p.text[start..end];
            from = end;
            if allowed.iter().any(|(a, w, _)| *a == path && *w == word) {
                continue;
            }
            let what = match (rust, p.kind) {
                (false, _) => "text",
                (true, Kind::Comment) => "a comment",
                (true, Kind::Str | Kind::Char) => "a literal",
                (true, Kind::Code) => "a name",
            };
            problems.push(format!(
                "{path}:{}: a letter outside ASCII in {what}: `{word}`",
                rust_text::line_of(text, p.start + at)
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
    fn sources_are_in_english() {
        let problems = super::run(&crate::workspace_root()).unwrap();
        assert!(
            problems.is_empty(),
            "\n{}\nWrite comments in English; a string whose bytes matter can use \\u{{...}} \
             escapes or go on ALLOWED in xtask/src/english.rs with the reason.\n",
            problems.join("\n")
        );
    }

    #[test]
    fn flags_letters_of_another_language() {
        let src = "// Fun\u{e7}\u{e3}o que l\u{ea}\nfn a() { let s = \"n\u{e3}o\"; }\n";
        let p = check("crates/x/src/lib.rs", src, true, &[]);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(p[0].contains(":1: a letter outside ASCII in a comment: `Fun\u{e7}\u{e3}o`"));
        assert!(p[2].contains(":2: a letter outside ASCII in a literal: `n\u{e3}o`"));
    }

    #[test]
    fn symbols_greek_letters_and_escapes_pass() {
        let src = "// a \u{2192} b \u{00d7} 2 \u{2014} 2\u{3c0} in 5 \u{b5}s\nconst S: &str = \"Caf\\u{e9}\";\n";
        assert!(check("crates/x/src/lib.rs", src, true, &[]).is_empty());
    }

    #[test]
    fn other_text_files_are_checked_whole() {
        let p = check("crates/x/shader.hlsl", "// cor do c\u{e9}u\n", false, &[]);
        assert_eq!(p.len(), 1, "{p:?}");
    }

    #[test]
    fn the_allowlist_names_a_word_in_a_file() {
        let src = "// Caf\u{e9}\n";
        let allowed = [("crates/x/src/lib.rs", "Caf\u{e9}", "a test of text")];
        assert!(check("crates/x/src/lib.rs", src, true, &allowed).is_empty());
        assert_eq!(check("crates/y/src/lib.rs", src, true, &allowed).len(), 1);
    }
}
