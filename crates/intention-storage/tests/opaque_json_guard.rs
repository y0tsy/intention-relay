#![allow(
    clippy::expect_used,
    reason = "Source guards use expect for precise test diagnostics."
)]

// The D-07 storage boundary guard scans source text, so it lives in this
// integration target instead of the library test module: the crate's
// coverage denominator stays about the library the guard protects. R44
// widened it from a `pub`-line scan to a struct-body parser that resolves
// same-crate aliases and fails closed.

use std::path::{Path, PathBuf};

/// One named field declaration recovered from a struct body.
#[derive(Debug, Eq, PartialEq)]
struct FieldDeclaration {
    declaration: String,
    name: String,
    field_type: String,
}

/// Reports whether one character continues a Rust identifier.
fn is_identifier_character(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Reports whether one text is a plain Rust identifier.
fn is_identifier(text: &str) -> bool {
    let mut characters = text.chars();
    characters
        .next()
        .is_some_and(|first| first.is_alphabetic() || first == '_')
        && characters.all(is_identifier_character)
}

/// Reports whether one line declares a struct, whatever the visibility.
fn line_declares_struct(line: &str) -> bool {
    let trimmed = line.trim_start();
    let rest = match trimmed.strip_prefix("pub") {
        Some(rest) => match rest.chars().next() {
            Some('(') => {
                let Some(end) = rest.find(')') else {
                    return false;
                };
                rest[end + 1..].trim_start()
            }
            Some(character) if character.is_whitespace() => rest.trim_start(),
            _ => return false,
        },
        None => trimmed,
    };
    rest == "struct"
        || rest.starts_with("struct ")
        || rest.starts_with("struct<")
        || rest.starts_with("struct(")
}

/// Returns the byte offset of the next struct declaration at or after
/// `from`.
fn next_struct_start(source: &str, from: usize) -> Option<usize> {
    let mut offset = from;
    for line in source[from..].split_inclusive('\n') {
        if line_declares_struct(line) {
            return Some(offset + (line.len() - line.trim_start().len()));
        }
        offset += line.len();
    }
    None
}

/// Returns every braced struct body in one source text. Comments inside a
/// body are skipped by the field parser, and a unit, tuple, or unbraced
/// struct declaration ends at its own semicolon.
fn struct_bodies(source: &str) -> Vec<String> {
    let mut bodies = Vec::new();
    let mut cursor = 0;
    while let Some(start) = next_struct_start(source, cursor) {
        let Some(open_offset) = source[start..].find('{') else {
            break;
        };
        let open = start + open_offset;
        if let Some(semicolon) = source[start..]
            .find(';')
            .filter(|semicolon| start + semicolon < open)
        {
            cursor = start + semicolon + 1;
            continue;
        }
        let Some((body, close)) = matching_brace(source, open) else {
            break;
        };
        bodies.push(body);
        cursor = close + 1;
    }
    bodies
}

/// Returns the body between one opening brace and its matching closing
/// brace, skipping line and nested block comments.
fn matching_brace(source: &str, open: usize) -> Option<(String, usize)> {
    let mut depth = 0_i32;
    let mut index = open;
    while index < source.len() {
        let current = source[index..].chars().next()?;
        let next = source[index + current.len_utf8()..].chars().next();
        if current == '/' && next == Some('/') {
            while index < source.len() && source.as_bytes()[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if current == '/' && next == Some('*') {
            let mut nesting = 1;
            index += 2;
            while index < source.len() && nesting > 0 {
                let inner = source[index..].chars().next()?;
                let after = source[index + inner.len_utf8()..].chars().next();
                if inner == '/' && after == Some('*') {
                    nesting += 1;
                    index += 2;
                } else if inner == '*' && after == Some('/') {
                    nesting -= 1;
                    index += 2;
                } else {
                    index += inner.len_utf8();
                }
            }
            continue;
        }
        if current == '{' {
            depth += 1;
        }
        if current == '}' {
            depth -= 1;
            if depth == 0 {
                return Some((source[open + 1..index].to_owned(), index));
            }
        }
        index += current.len_utf8();
    }
    None
}

/// Parses every named field declaration out of one struct body text,
/// splitting at top-level commas so a type spanning lines is still one
/// declaration.
fn parse_struct_fields(body: &str, fields: &mut Vec<FieldDeclaration>) {
    let mut depth = 0_i32;
    let mut declaration = String::new();
    let mut index = 0;
    while index < body.len() {
        let current = body[index..]
            .chars()
            .next()
            .expect("a struct body always has a character at a boundary");
        let next = body[index + current.len_utf8()..].chars().next();
        if current == '/' && next == Some('/') {
            while index < body.len() && body.as_bytes()[index] != b'\n' {
                index += 1;
            }
            continue;
        }
        if current == '/' && next == Some('*') {
            let mut nesting = 1;
            index += 2;
            while index < body.len() && nesting > 0 {
                let inner = body[index..]
                    .chars()
                    .next()
                    .expect("a block comment always has a character at a boundary");
                let after = body[index + inner.len_utf8()..].chars().next();
                if inner == '/' && after == Some('*') {
                    nesting += 1;
                    index += 2;
                } else if inner == '*' && after == Some('/') {
                    nesting -= 1;
                    index += 2;
                } else {
                    index += inner.len_utf8();
                }
            }
            declaration.push(' ');
            continue;
        }
        match current {
            '<' | '(' | '[' | '{' => {
                depth += 1;
                declaration.push(current);
            }
            '>' | ')' | ']' | '}' => {
                depth -= 1;
                declaration.push(current);
            }
            ',' if depth <= 0 => {
                push_field_declaration(&declaration, fields);
                declaration.clear();
            }
            _ => declaration.push(current),
        }
        index += current.len_utf8();
    }
    push_field_declaration(&declaration, fields);
}

/// Pushes one parsed field declaration, ignoring fragments that are not a
/// named field with any visibility.
fn push_field_declaration(declaration: &str, fields: &mut Vec<FieldDeclaration>) {
    let mut text = declaration.trim();
    while text.starts_with("#[") {
        let mut depth = 0_i32;
        let mut end = None;
        for (offset, character) in text.char_indices() {
            match character {
                '[' => depth += 1,
                ']' => {
                    depth -= 1;
                    if depth == 0 {
                        end = Some(offset);
                        break;
                    }
                }
                _ => {}
            }
        }
        let Some(end) = end else {
            return;
        };
        text = text[end + 1..].trim_start();
    }
    if let Some(rest) = text.strip_prefix("pub") {
        match rest.chars().next() {
            Some('(') => {
                let Some(end) = rest.find(')') else {
                    return;
                };
                text = rest[end + 1..].trim_start();
            }
            Some(character) if character.is_whitespace() => {
                text = rest.trim_start();
            }
            _ => {}
        }
    }
    let Some((name, field_type)) = text.split_once(':') else {
        return;
    };
    let name = name.trim();
    let field_type = field_type.trim();
    if !is_identifier(name) || field_type.is_empty() {
        return;
    }
    fields.push(FieldDeclaration {
        declaration: format!("{name}: {field_type}"),
        name: name.to_owned(),
        field_type: field_type.to_owned(),
    });
}

/// Returns every `type Name = Target;` alias declared in one source text.
fn type_aliases(source: &str) -> Vec<(String, String)> {
    let mut aliases = Vec::new();
    let mut rest = source;
    while let Some(position) = rest.find("type ") {
        let before = rest[..position].chars().next_back();
        let on_boundary = before.is_none_or(|character| !is_identifier_character(character));
        rest = &rest[position + "type ".len()..];
        if !on_boundary {
            continue;
        }
        let Some((name, declared)) = rest.split_once('=') else {
            break;
        };
        let name = name.trim();
        if !is_identifier(name) {
            continue;
        }
        let Some((target, after)) = declared.split_once(';') else {
            break;
        };
        let target = target.trim();
        if !target.is_empty() {
            aliases.push((name.to_owned(), target.to_owned()));
        }
        rest = after;
    }
    aliases
}

/// Replaces whole-identifier occurrences of one alias inside a type text.
fn replace_identifier(text: &str, name: &str, replacement: &str) -> String {
    let mut result = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(position) = rest.find(name) {
        let before = rest[..position].chars().next_back();
        let after = rest[position + name.len()..].chars().next();
        let on_boundaries = before.is_none_or(|character| !is_identifier_character(character))
            && after.is_none_or(|character| !is_identifier_character(character));
        result.push_str(&rest[..position]);
        result.push_str(if on_boundaries { replacement } else { name });
        rest = &rest[position + name.len()..];
    }
    result.push_str(rest);
    result
}

/// Expands same-crate type aliases inside one field type so a carrier
/// hidden behind an alias is classified by its target type.
fn expand_aliases(field_type: &str, aliases: &[(String, String)]) -> String {
    let mut expanded = field_type.to_owned();
    for _ in 0..8 {
        let mut next = expanded.clone();
        for (name, target) in aliases {
            next = replace_identifier(&next, name, target);
        }
        if next == expanded {
            break;
        }
        expanded = next;
    }
    expanded
}

/// Returns the struct field declarations in one source text that are
/// JSON-shaped carriers, whatever the field visibility: a field whose type
/// resolves to a JSON-crate value, or a field whose name contains `json`
/// and whose type resolves to a string type.
fn json_shaped_field_declarations(source: &str) -> Vec<String> {
    json_shaped_field_declarations_with(source, &type_aliases(source))
}

/// Returns the JSON-shaped carrier declarations when the alias set of the
/// surrounding crate is already known.
fn json_shaped_field_declarations_with(source: &str, aliases: &[(String, String)]) -> Vec<String> {
    // The JSON needles are assembled at runtime so this guard's own source
    // can neither satisfy nor trip the scan it performs.
    let json_fragment = ["j", "son"].concat();
    let json_crate = ["serde_", "json"].concat();
    let string_types = ["String", "str"];
    let mut declarations = Vec::new();
    for body in struct_bodies(source) {
        let mut fields = Vec::new();
        parse_struct_fields(&body, &mut fields);
        for field in fields {
            let field_type = expand_aliases(&field.field_type, aliases);
            let json_named = field.name.to_ascii_lowercase().contains(&json_fragment);
            let string_carrier =
                json_named && string_types.iter().any(|kind| field_type.contains(kind));
            if string_carrier || field_type.contains(&json_crate) {
                declarations.push(field.declaration);
            }
        }
    }
    declarations
}

/// Collects every Rust source under `root`, failing closed when any part
/// of the subtree cannot be read: an unreadable directory, entry, or file
/// aborts the scan instead of silently shrinking it. A `target` directory
/// is never part of the source surface.
fn rust_sources(root: &Path) -> Result<Vec<PathBuf>, String> {
    let mut sources = Vec::new();
    let mut pending = vec![root.to_path_buf()];
    while let Some(directory) = pending.pop() {
        let entries = std::fs::read_dir(&directory).map_err(|error| {
            format!(
                "unreadable source directory {}: {error}",
                directory.display()
            )
        })?;
        for entry in entries {
            let entry = entry
                .map_err(|error| format!("unreadable entry in {}: {error}", directory.display()))?;
            let path = entry.path();
            let file_type = entry
                .file_type()
                .map_err(|error| format!("unreadable entry type {}: {error}", path.display()))?;
            if file_type.is_dir() {
                if path.file_name() == Some(std::ffi::OsStr::new("target")) {
                    continue;
                }
                pending.push(path);
                continue;
            }
            if path.extension().is_some_and(|extension| extension == "rs") {
                sources.push(path);
            }
        }
    }
    sources.sort();
    Ok(sources)
}

/// Scans every Rust source under `root` and returns the number of scanned
/// files plus every JSON-shaped carrier field declaration on the crate's
/// DTO surface, resolving aliases across the whole tree and failing closed
/// when a source cannot be read.
fn scan_storage_source_tree(root: &Path) -> Result<(usize, Vec<String>), String> {
    let sources = rust_sources(root)?;
    let scanned = sources.len();
    let mut texts = Vec::with_capacity(scanned);
    for source in &sources {
        let text = std::fs::read_to_string(source)
            .map_err(|error| format!("unreadable source {}: {error}", source.display()))?;
        texts.push((source.clone(), text));
    }
    let mut aliases = Vec::new();
    for (_, text) in &texts {
        aliases.extend(type_aliases(text));
    }
    let mut offenses = Vec::new();
    for (source, text) in &texts {
        for declaration in json_shaped_field_declarations_with(text, &aliases) {
            offenses.push(format!("{}: {declaration}", source.display()));
        }
    }
    Ok((scanned, offenses))
}

/// D-07 boundary guard: no storage DTO may declare a JSON-shaped carrier
/// field. The guard scans every Rust source of this crate, whatever the
/// directory, parses each struct body whatever the field visibility and
/// layout, resolves same-crate type aliases, and fails closed when the
/// source subtree cannot be read.
///
/// A carrier is a field whose type names the JSON crate (its untyped
/// value, its map, or any other shape) or a field whose name contains
/// `json` and whose type resolves to a string. Documented limits: a string
/// carrier whose name carries no `json` marker cannot be distinguished
/// from a legitimate typed string field by source text alone, a generic
/// alias target is not substituted, and macro-generated fields are
/// invisible to the scan.
#[test]
fn storage_dto_surface_declares_no_opaque_json_string_field() {
    let root = Path::new(env!("CARGO_MANIFEST_DIR"));
    let (scanned, offenses) =
        scan_storage_source_tree(root).expect("the storage source tree is readable");
    assert!(
        scanned > 0,
        "the guard must scan at least one storage source file"
    );
    assert!(
        offenses.is_empty(),
        "the storage boundary must not declare JSON-shaped carrier fields: {offenses:#?}"
    );
}

#[test]
fn opaque_json_guard_flags_every_json_shaped_carrier_field() {
    let json = ["j", "son"].concat();
    let json_crate = ["serde_", "json"].concat();
    for evasion in [
        format!("struct Fixture {{ pub {json}: String, }}"),
        format!("struct Fixture {{ pub payload_{json}_text: String, }}"),
        format!("struct Fixture {{ pub payload_{json}: Box<str>, }}"),
        format!("struct Fixture {{ pub payload_{json}: &'static str, }}"),
        format!("struct Fixture {{ pub payload_{json}: Option<Box<str>>, }}"),
        format!("struct Fixture {{ pub(crate) payload_{json}: String, }}"),
        format!("struct Fixture {{ payload_{json}: String, }}"),
        format!("struct Fixture {{ pub payload_{json}:\n    String, }}"),
        format!("struct Fixture {{ pub metadata: {json_crate}::Value, }}"),
        format!("struct Fixture {{ pub metadata: {json_crate}::Map<String, String>, }}"),
        format!("type JsonText = String;\nstruct Fixture {{ pub payload_{json}: JsonText, }}"),
        format!("type JsonText = String;\nstruct Fixture {{ pub payload_{json}: Vec<JsonText>, }}"),
    ] {
        let offenses = json_shaped_field_declarations(&evasion);
        assert_eq!(
            offenses.len(),
            1,
            "the guard must flag `{evasion}`: {offenses:#?}"
        );
    }
    for clean in [
        "struct Fixture { pub selection: ProviderSelectionV1 }",
        "struct Fixture { pub revision_id: String }",
        "struct Fixture { pub profiles: Vec<ProviderProfileRevisionV1> }",
        "struct Fixture { pub payload: Box<str> }",
        "struct Fixture { pub metadata: HashMap<String, String> }",
    ] {
        assert!(
            json_shaped_field_declarations(clean).is_empty(),
            "the guard must not flag the clean typed field `{clean}`"
        );
    }
}

#[test]
fn opaque_json_guard_scans_sources_outside_src() {
    let json = ["j", "son"].concat();
    let root = std::env::temp_dir().join(format!(
        "intention-storage-opaque-json-guard-{}",
        std::process::id()
    ));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("src")).expect("fixture source directory creates");
    std::fs::create_dir_all(root.join("tests")).expect("fixture test directory creates");
    std::fs::write(
        root.join("src/alias.rs"),
        "type JsonText = String;\nstruct Clean { pub revision_id: String }\n",
    )
    .expect("fixture alias writes");
    std::fs::write(
        root.join("tests/evasion.rs"),
        format!("struct Carrier {{ pub payload_{json}: JsonText }}\n"),
    )
    .expect("fixture evasion writes");
    let (scanned, offenses) =
        scan_storage_source_tree(&root).expect("the fixture tree is readable");
    assert_eq!(scanned, 2);
    assert_eq!(
        offenses.len(),
        1,
        "a carrier outside src must be scanned and its alias across files must resolve: {offenses:#?}"
    );
    assert!(
        offenses[0].contains("evasion.rs"),
        "the offense names the non-src source: {offenses:#?}"
    );
    let _ = std::fs::remove_dir_all(&root);
}

#[test]
fn opaque_json_guard_fails_closed_on_an_unreadable_source_tree() {
    let missing = Path::new(env!("CARGO_MANIFEST_DIR")).join("src-does-not-exist");
    assert!(
        scan_storage_source_tree(&missing).is_err(),
        "an unreadable source subtree must fail the guard closed"
    );
}
