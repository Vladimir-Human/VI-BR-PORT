//! Links in the documentation point at files the repository has: every
//! relative link target of a Markdown file (`[text](path)`, `![alt](path)`,
//! `[name]: path`, and `src="..."` or `href="..."` in HTML) names a file or
//! folder that is committed (or about to be). Links to the web and to anchors
//! of the same page are not checked; code blocks and inline code are skipped.

use std::collections::BTreeSet;
use std::path::Path;

use anyhow::Result;

pub fn run(root: &Path) -> Result<Vec<String>> {
    let files = crate::policy::list_files(root, false)?;
    let set: BTreeSet<&str> = files.iter().map(String::as_str).collect();
    let mut problems = Vec::new();
    for path in files.iter().filter(|p| p.ends_with(".md")) {
        let Ok(text) = std::fs::read_to_string(root.join(path)) else {
            continue;
        };
        problems.extend(check(path, &text, &set));
    }
    Ok(problems)
}

/// Whether `path` (normalised, from the root) is a file of `files` or a
/// folder holding one.
fn exists(files: &BTreeSet<&str>, path: &str) -> bool {
    let path = path.trim_end_matches('/');
    if path.is_empty() || files.contains(path) {
        return true;
    }
    let folder = format!("{path}/");
    files
        .range(folder.as_str()..)
        .next()
        .is_some_and(|f| f.starts_with(&folder))
}

/// `target` resolved against the folder of `from`, `.` and `..` removed;
/// `None` if it climbs out of the repository.
fn resolve(from: &str, target: &str) -> Option<String> {
    let mut parts: Vec<&str> = match target.strip_prefix('/') {
        Some(_) => Vec::new(),
        None => from.split('/').collect::<Vec<_>>().split_last()?.1.to_vec(),
    };
    for part in target.split('/') {
        match part {
            "" | "." => {}
            ".." => {
                parts.pop()?;
            }
            p => parts.push(p),
        }
    }
    Some(parts.join("/"))
}

/// The link targets of one Markdown line, inline code left out.
fn targets(line: &str) -> Vec<String> {
    // Inline code spans: the text between pairs of backticks.
    let kept: String = line
        .split('`')
        .enumerate()
        .map(|(i, part)| if i % 2 == 0 { part } else { " " })
        .collect();
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(n) = kept[from..].find("](") {
        let rest = &kept[from + n + 2..];
        let rest = rest.strip_prefix('<').unwrap_or(rest);
        let end = rest
            .find(|c: char| c == ')' || c == '>' || c.is_whitespace())
            .unwrap_or(rest.len());
        out.push(rest[..end].to_owned());
        from = kept.len() - rest.len() + end;
    }
    for attr in ["src=\"", "href=\""] {
        for part in kept.split(attr).skip(1) {
            out.push(part.split('"').next().unwrap_or("").to_owned());
        }
    }
    // A reference definition: `[name]: target`.
    let trimmed = kept.trim_start();
    if trimmed.starts_with('[')
        && let Some(close) = trimmed.find("]:")
    {
        let t = trimmed[close + 2..].split_whitespace().next().unwrap_or("");
        out.push(t.trim_start_matches('<').trim_end_matches('>').to_owned());
    }
    out
}

/// All problems with one Markdown file. Pure, for testing.
pub fn check(path: &str, text: &str, files: &BTreeSet<&str>) -> Vec<String> {
    let mut problems = Vec::new();
    let mut fence: Option<&str> = None;
    for (n, line) in text.lines().enumerate() {
        let t = line.trim_start();
        if let Some(f) = fence {
            if t.starts_with(f) {
                fence = None;
            }
            continue;
        }
        if t.starts_with("```") || t.starts_with("~~~") {
            fence = Some(&t[..3]);
            continue;
        }
        for target in targets(line) {
            let target = target.as_str();
            if target.is_empty() || target.starts_with('#') || target.contains(':') {
                continue; // an anchor, or a scheme (https:, mailto:)
            }
            let file = target.split(['#', '?']).next().unwrap_or(target);
            let file = file.replace("%20", " ");
            match resolve(path, &file) {
                Some(p) if exists(files, &p) => {}
                Some(p) => problems.push(format!(
                    "{path}:{}: the link `{target}` points at {p}, which is not in the repository",
                    n + 1
                )),
                None => problems.push(format!(
                    "{path}:{}: the link `{target}` points outside the repository",
                    n + 1
                )),
            }
        }
    }
    problems
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::check;

    /// Enforces the rule wherever tests run.
    #[test]
    fn documentation_links_point_at_files_that_exist() {
        let problems = super::run(&crate::workspace_root()).unwrap();
        assert!(problems.is_empty(), "\n{}\n", problems.join("\n"));
    }

    const FILES: &[&str] = &["LICENSE", "README.md", "docs/a.md", "docs/img/x.svg"];

    fn run(path: &str, text: &str) -> Vec<String> {
        let files: BTreeSet<&str> = FILES.iter().copied().collect();
        check(path, text, &files)
    }

    #[test]
    fn links_that_resolve_pass() {
        let text = "See [a](a.md#part), ![x](img/x.svg \"title\"), [up](../README.md), \
                    [root](/LICENSE), [folder](img/), [web](https://example.com), [here](#top).\n\
                    <img src=\"img/x.svg\">\n[ref]: <../LICENSE>\n";
        let p = run("docs/b.md", text);
        assert!(p.is_empty(), "{p:?}");
    }

    #[test]
    fn broken_links_fail() {
        let text = "[gone](gone.md) and ![pic](img/y.svg)\n[ref]: ../../outside.md\n";
        let p = run("docs/b.md", text);
        assert_eq!(p.len(), 3, "{p:?}");
        assert!(p[0].contains("docs/b.md:1: the link `gone.md` points at docs/gone.md"));
        assert!(p[2].contains("outside the repository"));
    }

    #[test]
    fn code_is_not_read_for_links() {
        let text = "`[x](gone.md)` and\n```\n[y](gone.md)\n```\n[a](a.md)\n";
        let p = run("docs/b.md", text);
        assert!(p.is_empty(), "{p:?}");
    }
}
