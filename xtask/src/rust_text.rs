//! Just enough of a Rust lexer for the repository checks: it tells comments
//! and literals from code, and finds the items only test builds compile. It
//! works on the text alone, so a check needs no build and no parser crate.
//!
//! What it understands: line and (nested) block comments, doc comments,
//! string literals with their `b`/`c`/`r#` forms, char literals as opposed to
//! lifetimes and labels, and `#[cfg(...)]` predicates made of `test`, `all`,
//! `any` and `not`. Anything else counts as code.

use std::ops::Range;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Kind {
    Code,
    /// A comment of either form, doc comments included.
    Comment,
    /// A string literal, delimiters and prefix included.
    Str,
    /// A char or byte literal, quotes included.
    Char,
}

#[derive(Clone, Copy, Debug)]
pub struct Piece<'a> {
    pub kind: Kind,
    pub text: &'a str,
    /// Byte offset of `text` in the source.
    pub start: usize,
}

impl<'a> Piece<'a> {
    /// What a string literal says, without prefix, hashes and quotes
    /// (escapes are left as they are).
    pub fn str_value(&self) -> Option<&'a str> {
        if self.kind != Kind::Str {
            return None;
        }
        let open = self.text.find('"')?;
        let close = self.text.rfind('"')?;
        (close > open).then(|| &self.text[open + 1..close])
    }
}

fn is_ident(c: char) -> bool {
    c.is_alphanumeric() || c == '_'
}

/// The source cut into pieces that together cover it exactly, in order.
pub fn pieces<'a>(src: &'a str) -> Vec<Piece<'a>> {
    let bytes = src.as_bytes();
    let mut out = Vec::new();
    let mut code_from = 0;
    let mut i = 0;
    let push = |out: &mut Vec<Piece<'a>>, code_from: &mut usize, kind, from: usize, to: usize| {
        if *code_from < from {
            out.push(Piece {
                kind: Kind::Code,
                text: &src[*code_from..from],
                start: *code_from,
            });
        }
        out.push(Piece {
            kind,
            text: &src[from..to],
            start: from,
        });
        *code_from = to;
    };
    while i < bytes.len() {
        let rest = &bytes[i..];
        let prev_ident = src[..i].chars().next_back().is_some_and(is_ident);
        if rest.starts_with(b"//") {
            let end = src[i..].find('\n').map_or(src.len(), |n| i + n);
            push(&mut out, &mut code_from, Kind::Comment, i, end);
            i = end;
        } else if rest.starts_with(b"/*") {
            let (mut depth, mut j) = (0usize, i);
            while j < bytes.len() {
                if bytes[j..].starts_with(b"/*") {
                    depth += 1;
                    j += 2;
                } else if bytes[j..].starts_with(b"*/") {
                    depth -= 1;
                    j += 2;
                    if depth == 0 {
                        break;
                    }
                } else {
                    j += 1;
                }
            }
            push(&mut out, &mut code_from, Kind::Comment, i, j);
            i = j;
        } else if let Some(end) = (!prev_ident).then(|| raw_string(bytes, i)).flatten() {
            push(&mut out, &mut code_from, Kind::Str, i, end);
            i = end;
        } else if bytes[i] == b'"' {
            // A `b` or `c` prefix stays in the code before it; harmless.
            let mut j = i + 1;
            while j < bytes.len() && bytes[j] != b'"' {
                j += if bytes[j] == b'\\' { 2 } else { 1 };
            }
            let end = (j + 1).min(bytes.len());
            push(&mut out, &mut code_from, Kind::Str, i, end);
            i = end;
        } else if bytes[i] == b'\'' {
            match char_literal(src, i) {
                Some(end) => {
                    push(&mut out, &mut code_from, Kind::Char, i, end);
                    i = end;
                }
                None => i += 1, // a lifetime or a label
            }
        } else {
            i += src[i..].chars().next().map_or(1, char::len_utf8);
        }
    }
    if code_from < src.len() {
        out.push(Piece {
            kind: Kind::Code,
            text: &src[code_from..],
            start: code_from,
        });
    }
    out
}

/// The end of a raw string (`r"..."`, `br#"..."#`, `cr##"..."##`) starting at
/// `i`, if one does.
fn raw_string(bytes: &[u8], i: usize) -> Option<usize> {
    let mut j = i;
    if matches!(bytes.get(j), Some(b'b' | b'c')) {
        j += 1;
    }
    if bytes.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let hashes = bytes[j..].iter().take_while(|&&b| b == b'#').count();
    j += hashes;
    if bytes.get(j) != Some(&b'"') {
        return None;
    }
    j += 1;
    while j < bytes.len() {
        if bytes[j] == b'"'
            && bytes[j + 1..]
                .iter()
                .take(hashes)
                .filter(|&&b| b == b'#')
                .count()
                == hashes
        {
            return Some(j + 1 + hashes);
        }
        j += 1;
    }
    Some(bytes.len())
}

/// The end of a char literal starting at the quote at `i`, or `None` for a
/// lifetime or label (`'a`, `'outer:`).
fn char_literal(src: &str, i: usize) -> Option<usize> {
    let mut chars = src[i + 1..].char_indices();
    let (_, first) = chars.next()?;
    if first == '\\' {
        // An escape: the literal ends at the next quote after the escaped
        // character (`'\''`, `'\\'`, `'\u{e9}'`).
        chars.next()?;
        let (at, _) = chars.find(|&(_, c)| c == '\'')?;
        return Some(i + 1 + at + 1);
    }
    let (at, second) = chars.next()?;
    (second == '\'').then_some(i + 1 + at + 1)
}

/// The source with every comment and literal blanked out (each byte but a
/// newline turned into a space), so offsets and line numbers stay the same and
/// braces or `#[` inside them are not seen as code.
pub fn code_only(src: &str) -> String {
    let mut out = String::with_capacity(src.len());
    for p in pieces(src) {
        if p.kind == Kind::Code {
            out.push_str(p.text);
        } else {
            out.extend(p.text.bytes().map(|b| if b == b'\n' { '\n' } else { ' ' }));
        }
    }
    out
}

/// The 1-based line of byte offset `at`.
pub fn line_of(src: &str, at: usize) -> usize {
    src.as_bytes()[..at].iter().filter(|&&b| b == b'\n').count() + 1
}

/// What only test builds compile in one file.
#[derive(Debug, Default)]
pub struct TestOnly {
    /// The whole file (`#![cfg(test)]` at its top).
    pub file: bool,
    /// Byte ranges of `#[cfg(test)]` items written in the file.
    pub ranges: Vec<Range<usize>>,
    /// Modules the file declares as `#[cfg(test)] mod name;`, kept in files
    /// of their own.
    pub modules: Vec<String>,
}

impl TestOnly {
    pub fn contains(&self, at: usize) -> bool {
        self.file || self.ranges.iter().any(|r| r.contains(&at))
    }
}

/// Finds the test-only parts of `src`.
pub fn test_only(src: &str) -> TestOnly {
    let code = code_only(src);
    let b = code.as_bytes();
    let mut out = TestOnly::default();
    let mut i = 0;
    while let Some(n) = code[i..].find('#') {
        let at = i + n;
        let inner = b.get(at + 1) == Some(&b'!');
        let open = at + 1 + usize::from(inner);
        let Some(close) = (b.get(open) == Some(&b'['))
            .then(|| matching(b, open))
            .flatten()
        else {
            i = at + 1;
            continue;
        };
        let attr = code[open + 1..close].trim();
        let is_test = attr
            .strip_prefix("cfg")
            .map(str::trim_start)
            .and_then(|p| p.strip_prefix('('))
            .and_then(|p| p.strip_suffix(')'))
            .is_some_and(|pred| cfg_test_only(&mut Tokens::new(pred)));
        if !is_test {
            i = close + 1;
            continue;
        }
        if inner {
            out.file = true;
            return out;
        }
        let (item_end, module) = item(&code, close + 1);
        out.ranges.push(at..item_end);
        out.modules.extend(module);
        i = item_end.max(close + 1);
    }
    out
}

/// The offset of the bracket that closes the one at `open`.
fn matching(b: &[u8], open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (j, &c) in b.iter().enumerate().skip(open) {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth -= 1;
                if depth == 0 {
                    return Some(j);
                }
            }
            _ => {}
        }
    }
    None
}

const ITEM_WORDS: &[&str] = &[
    "fn",
    "impl",
    "mod",
    "struct",
    "enum",
    "union",
    "use",
    "static",
    "const",
    "type",
    "trait",
    "extern",
    "unsafe",
    "async",
    "macro_rules",
    "thread_local",
];

/// The end of the item (or statement, field, arm) that starts after the
/// attribute ending at `from`, and the module name if it is `mod name;`.
fn item(code: &str, from: usize) -> (usize, Option<String>) {
    let b = code.as_bytes();
    // Further attributes on the same item.
    let mut start = from;
    loop {
        let s = start + code[start..].len() - code[start..].trim_start().len();
        if b.get(s) == Some(&b'#') && b.get(s + 1) == Some(&b'[') {
            match matching(b, s + 1) {
                Some(c) => start = c + 1,
                None => return (code.len(), None),
            }
        } else {
            start = s;
            break;
        }
    }
    let words: Vec<&str> = code[start..]
        .split(|c: char| !is_ident(c))
        .filter(|w| !w.is_empty())
        .take(3)
        .collect();
    let first = match words.as_slice() {
        ["pub", "crate" | "super" | "self" | "in", w, ..] | ["pub", w, ..] => *w,
        [w, ..] => *w,
        [] => "",
    };
    let is_item = ITEM_WORDS.contains(&first);
    let mut depth = 0i32;
    for (j, &c) in b.iter().enumerate().skip(start) {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' => depth -= 1,
            b'}' => {
                depth -= 1;
                if depth == 0 {
                    return (j + 1, None);
                }
            }
            b';' if depth == 0 => {
                let text = &code[start..j];
                let module = text
                    .split_whitespace()
                    .skip_while(|w| *w != "mod")
                    .nth(1)
                    .filter(|_| is_item && first == "mod")
                    .map(str::to_owned);
                return (j + 1, module);
            }
            b',' if depth == 0 && !is_item => return (j, None),
            _ => {}
        }
        if depth < 0 {
            return (j, None);
        }
    }
    (code.len(), None)
}

/// The tokens of a `cfg` predicate: words and the punctuation `( ) , =`
/// (string contents are already blanked).
struct Tokens<'a> {
    rest: &'a str,
}

impl<'a> Tokens<'a> {
    fn new(s: &'a str) -> Self {
        Self { rest: s }
    }

    fn peek(&self) -> Option<&'a str> {
        let s = self.rest.trim_start();
        let c = s.chars().next()?;
        if matches!(c, '(' | ')' | ',' | '=') {
            return Some(&s[..1]);
        }
        let end = s.find(|c: char| !is_ident(c)).unwrap_or(s.len());
        Some(&s[..if end == 0 { c.len_utf8() } else { end }])
    }

    fn next(&mut self) -> Option<&'a str> {
        let t = self.peek()?;
        let s = self.rest.trim_start();
        self.rest = &s[t.len()..];
        Some(t)
    }
}

/// Whether a `cfg` predicate holds only in test builds: `test` does, `all`
/// does when any of its parts does, `any` when every part does, and `not`
/// never (it holds in builds that are not tests).
fn cfg_test_only(t: &mut Tokens) -> bool {
    let Some(word) = t.next() else {
        return false;
    };
    match word {
        "all" | "any" | "not" if t.peek() == Some("(") => {
            t.next();
            let mut parts = Vec::new();
            while !matches!(t.peek(), Some(")") | None) {
                parts.push(cfg_test_only(t));
                if t.peek() == Some(",") {
                    t.next();
                }
            }
            t.next();
            match word {
                "all" => parts.iter().any(|&p| p),
                "any" => !parts.is_empty() && parts.iter().all(|&p| p),
                _ => false,
            }
        }
        _ => {
            if t.peek() == Some("=") {
                t.next();
                // The value: a blanked string, so nothing, or a word.
                if !matches!(t.peek(), Some(")" | ",") | None) {
                    t.next();
                }
            }
            word == "test"
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kinds(src: &str) -> Vec<(Kind, &str)> {
        pieces(src).into_iter().map(|p| (p.kind, p.text)).collect()
    }

    #[test]
    fn pieces_cover_the_source() {
        let src = "fn a<'a>(x: &'a str) -> char { /* b /* nested */ c */ 'x' } // end\n\"s\\\"t\" r#\"raw \" here\"# '\\'' b'\\n'";
        let all: String = pieces(src).iter().map(|p| p.text).collect();
        assert_eq!(all, src);
        let k = kinds(src);
        assert!(k.contains(&(Kind::Comment, "/* b /* nested */ c */")));
        assert!(k.contains(&(Kind::Comment, "// end")));
        assert!(k.contains(&(Kind::Char, "'x'")));
        assert!(k.contains(&(Kind::Str, "\"s\\\"t\"")));
        assert!(k.contains(&(Kind::Str, "r#\"raw \" here\"#")));
        assert!(k.contains(&(Kind::Char, "'\\''")));
        assert!(k.contains(&(Kind::Char, "'\\n'")));
        assert!(
            !k.iter().any(|(_, t)| t.starts_with("'a")),
            "a lifetime is code: {k:?}"
        );
    }

    #[test]
    fn an_identifier_ending_in_r_is_not_a_raw_string() {
        let k = kinds("let ptr = for_r\"x\";");
        assert!(k.contains(&(Kind::Str, "\"x\"")), "{k:?}");
    }

    #[test]
    fn str_value_strips_the_delimiters() {
        let p = pieces("f(br##\"VICTORY_X\"##)");
        let s: Vec<_> = p.iter().filter_map(Piece::str_value).collect();
        assert_eq!(s, ["VICTORY_X"]);
    }

    #[test]
    fn code_only_keeps_offsets() {
        let src = "a // \u{e9}\n\"{\" b";
        let code = code_only(src);
        assert_eq!(code.len(), src.len());
        assert!(!code.contains('{'));
        assert_eq!(line_of(src, src.find('b').unwrap()), 2);
    }

    #[test]
    fn cfg_predicates() {
        let only = |p: &str| cfg_test_only(&mut Tokens::new(p));
        assert!(only("test"));
        assert!(only("all(test, windows)"));
        assert!(only("all(windows, test)"));
        assert!(!only("any(windows, test)"));
        assert!(!only("all(windows, any(test, feature =        ))"));
        assert!(only("any(test, all(test, windows))"));
        assert!(!only("not(test)"));
        assert!(!only("windows"));
    }

    #[test]
    fn finds_test_only_items() {
        let src = "fn live() { a }\n#[cfg(test)]\nmod tests {\n    fn t() { \"}\" }\n}\nfn after() {}\n\
                   #[cfg(all(test, windows))]\n#[allow(dead_code)]\npub(crate) mod far;\n\
                   fn f() {\n    #[cfg(test)]\n    if x { return; }\n    live();\n}\n\
                   #[cfg(any(windows, test))]\nfn both() {}\n";
        let t = test_only(src);
        assert!(!t.file);
        assert_eq!(t.modules, ["far"]);
        let at = |s: &str| src.find(s).unwrap();
        assert!(t.contains(at("fn t()")));
        assert!(!t.contains(at("fn after")));
        assert!(!t.contains(at("fn live")));
        assert!(t.contains(at("return;")));
        assert!(!t.contains(at("live();")));
        assert!(!t.contains(at("fn both")));
    }

    #[test]
    fn a_test_only_field_ends_at_its_comma() {
        let src = "struct S {\n    #[cfg(test)]\n    a: u32,\n    b: u32,\n}";
        let t = test_only(src);
        assert!(t.contains(src.find("a: u32").unwrap()));
        assert!(!t.contains(src.find("b: u32").unwrap()));
    }

    #[test]
    fn an_inner_attribute_covers_the_file() {
        assert!(test_only("//! Tests.\n#![cfg(test)]\nfn x() {}").file);
        assert!(!test_only("#![allow(dead_code)]\nfn x() {}").file);
    }
}
