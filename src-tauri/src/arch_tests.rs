use std::{fs, io, path::Path};

#[derive(Clone, Copy)]
struct Token<'a> {
    text: &'a str,
    line: usize,
}

// Consume a literal as one token so its punctuation cannot change module depth.
fn literal_len(s: &str) -> Option<usize> {
    let body = s.strip_prefix(['b', 'c']).unwrap_or(s);
    let prefix = s.len() - body.len();
    if let Some(raw) = body.strip_prefix('r') {
        let hashes = raw.len() - raw.trim_start_matches('#').len();
        let content = raw[hashes..].strip_prefix('"')?;
        let end = format!("\"{}", &raw[..hashes]);
        return Some(s.len() - content.len() + content.find(&end)? + end.len());
    }
    let quote = match body.as_bytes().first()? {
        b'"' => b'"',
        // A lifetime has no closing apostrophe after its first character.
        b'\'' if body[1..].starts_with('\\') || body[1..].chars().nth(1) == Some('\'') => b'\'',
        _ => return None,
    };
    let mut bytes = body.bytes().enumerate().skip(1);
    while let Some((i, byte)) = bytes.next() {
        if byte == quote {
            return Some(prefix + i + 1);
        }
        if byte == b'\\' {
            bytes.next();
        }
    }
    None
}

fn block_comment_len(s: &str) -> usize {
    let mut rest = &s[2..];
    let mut depth = 1;
    while let Some(i) = rest.find(['/', '*']) {
        rest = &rest[i..];
        if rest.starts_with("/*") {
            depth += 1;
        } else if rest.starts_with("*/") {
            depth -= 1;
        } else {
            rest = &rest[1..];
            continue;
        }
        rest = &rest[2..];
        if depth == 0 {
            return s.len() - rest.len();
        }
    }
    s.len()
}

fn word_char(c: char) -> bool {
    c == '_' || c.is_alphanumeric() || !c.is_ascii() && !c.is_whitespace()
}

fn tokenize(mut s: &str) -> Vec<Token<'_>> {
    let mut tokens = Vec::new();
    let mut line = 1;
    while let Some(first) = s.chars().next() {
        let (len, keep) = if first.is_whitespace() {
            (s.len() - s.trim_start().len(), false)
        } else if s.starts_with("//") {
            (s.find('\n').unwrap_or(s.len()), false)
        } else if s.starts_with("/*") {
            (block_comment_len(s), false)
        } else if let Some(len) = literal_len(s) {
            (len, true)
        } else if word_char(first) {
            let word = s.strip_prefix("r#").unwrap_or(s);
            (s.len() - word.trim_start_matches(word_char).len(), true)
        } else if s.starts_with("::") {
            (2, true)
        } else {
            (first.len_utf8(), true)
        };
        let (part, rest) = s.split_at(len);
        if keep {
            tokens.push(Token { text: part, line });
        }
        line += part.bytes().filter(|&b| b == b'\n').count();
        s = rest;
    }
    tokens
}

struct Scan<'a> {
    file: &'a Path,
    layer: &'a str,
    depth: usize,
    violations: Vec<String>,
}

impl Scan<'_> {
    fn check(&mut self, path: &[&str], line: usize) {
        let mut parts = path.iter().copied().peekable();
        let root = match parts.next() {
            Some("tauri") => "tauri",
            Some("crate") => parts.next().unwrap_or("*"),
            Some(first @ ("self" | "super")) => {
                let mut up = usize::from(first == "super");
                while parts.next_if_eq(&"super").is_some() {
                    up += 1;
                }
                if up != self.depth {
                    return;
                }
                parts.next().unwrap_or("*")
            }
            _ => return,
        };
        let forbidden = match self.layer {
            "optics" => matches!(root, "service" | "agents" | "chat" | "tauri" | "*" | "self"),
            "service" => !matches!(root, "service" | "optics" | "compute" | "preferences"),
            _ => unreachable!(),
        };
        if forbidden {
            self.violations.push(format!(
                "src-tauri/src/{}:{line}  {}",
                self.file.display(),
                path.join("::")
            ));
        }
    }

    // A use tree shares its prefix across branches. For a grouped import, point
    // to the branch's line so multiple bad imports on separate lines stay distinct.
    fn path<'a>(&mut self, tokens: &mut &'a [Token<'a>], prefix: &mut Vec<&'a str>, import: bool) {
        let saved = prefix.len();
        let Some(start) = tokens.first() else { return };
        if start.text == "::" {
            *tokens = &tokens[1..];
        }
        while let Some((token, rest)) = tokens.split_first() {
            if import && token.text == "{" {
                *tokens = rest;
                while !tokens.is_empty() && tokens[0].text != "}" {
                    self.path(tokens, prefix, true);
                    if tokens.first().is_none_or(|t| t.text != ",") {
                        break;
                    }
                    *tokens = &tokens[1..];
                }
                if tokens.first().is_some_and(|t| t.text == "}") {
                    *tokens = &tokens[1..];
                }
                break;
            }
            let name = token.text.strip_prefix("r#").unwrap_or(token.text);
            if name != "*" && !name.chars().all(word_char) {
                break;
            }
            prefix.push(name);
            *tokens = rest;
            if rest.first().is_some_and(|t| t.text == "::")
                && rest.get(1).is_some_and(|t| t.text != "<")
            {
                *tokens = &rest[1..];
                continue;
            }
            self.check(prefix, start.line);
            if import && rest.first().is_some_and(|t| t.text == "as") && rest.len() >= 2 {
                *tokens = &rest[2..];
            }
            break;
        }
        prefix.truncate(saved);
    }
}

// File paths are relative to src/. foo.rs and foo/mod.rs have the same depth.
fn scan(file: &Path, source: &str) -> Vec<String> {
    let first = Path::new(file.iter().next().unwrap());
    let layer = first.file_stem().unwrap().to_str().unwrap();
    let mut scan = Scan {
        file,
        layer,
        depth: file.components().count() - usize::from(file.ends_with("mod.rs")),
        violations: Vec::new(),
    };
    let tokens = tokenize(source);
    let mut rest = tokens.as_slice();
    let mut braces = Vec::new();
    let mut prefix = Vec::new();
    while let Some((token, tail)) = rest.split_first() {
        let i = tokens.len() - rest.len();
        rest = tail;
        match token.text {
            "{" => {
                let module = i >= 2 && tokens[i - 2].text == "mod";
                braces.push(module);
                scan.depth += usize::from(module);
            }
            "}" => scan.depth -= usize::from(braces.pop() == Some(true)),
            "use" => scan.path(&mut rest, &mut prefix, true),
            "crate" if i > 0 && tokens[i - 1].text == "extern" => {
                scan.path(&mut rest, &mut prefix, true);
            }
            word if matches!(word, "crate" | "super" | "self" | "tauri" | "r#tauri")
                && tail.first().is_some_and(|t| t.text == "::") =>
            {
                rest = &tokens[i..];
                scan.path(&mut rest, &mut prefix, false);
            }
            _ => {}
        }
    }
    scan.violations
}

fn scan_dir(src: &Path, dir: &Path, violations: &mut Vec<String>) -> io::Result<()> {
    let mut entries = fs::read_dir(dir)?.collect::<io::Result<Vec<_>>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file = path.strip_prefix(src).unwrap();
        if dir == src
            && !matches!(
                file.to_str(),
                Some("optics" | "optics.rs" | "service" | "service.rs")
            )
        {
            continue;
        }
        if entry.file_type()?.is_dir() {
            scan_dir(src, &path, violations)?;
        } else if path.extension().is_some_and(|ext| ext == "rs") {
            violations.extend(scan(file, &fs::read_to_string(&path)?));
        }
    }
    Ok(())
}

#[test]
fn layer_dependencies() -> io::Result<()> {
    let src = Path::new(env!("CARGO_MANIFEST_DIR")).join("src");
    let mut violations = Vec::new();
    scan_dir(&src, &src, &mut violations)?;
    assert!(violations.is_empty(), "\n{}", violations.join("\n"));
    Ok(())
}

#[test]
fn layer_diagnostics_report_only_forbidden_paths() {
    let optics = r###"use crate::service;
/* crate::chat /* tauri::Builder */ crate::agents */
const EXAMPLE: &str = r##"crate::agents " tauri::Builder
}"##; // crate::service
#[cfg(test)] mod tests {
    use super::super::math; // still in optics
    fn check<'a>() { let _ = ('}', b'"', "escaped \" crate::chat"); }
    use super::super::super::{optics, agents as other};
    fn call() { crate /* comment */ ::chat::runtime(); }
    #[tauri::command] fn command() {}
}"###;
    assert_eq!(
        scan(Path::new("optics/bench.rs"), optics),
        [
            "src-tauri/src/optics/bench.rs:1  crate::service",
            "src-tauri/src/optics/bench.rs:8  super::super::super::agents",
            "src-tauri/src/optics/bench.rs:9  crate::chat::runtime",
            "src-tauri/src/optics/bench.rs:10  tauri::command",
        ]
    );
    let service = "use crate::{optics, compute, preferences, service::schema,
        future_layer::Thing, r#agents};
use super::chat;
use ::tauri as desktop;";
    assert_eq!(
        scan(Path::new("service.rs"), service),
        [
            "src-tauri/src/service.rs:2  crate::future_layer::Thing",
            "src-tauri/src/service.rs:2  crate::agents",
            "src-tauri/src/service.rs:3  super::chat",
            "src-tauri/src/service.rs:4  tauri",
        ]
    );
    assert_eq!(
        scan(Path::new("service/nested/mod.rs"), "use super::super::*;"),
        ["src-tauri/src/service/nested/mod.rs:1  super::super::*",]
    );
}
