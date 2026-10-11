# Patched revue 3.9.1

This directory is a vendored copy of the `revue` 3.9.1 crate
(<https://github.com/hawk90/revue>, MIT) selected by the root `Cargo.toml`
through `[patch.crates-io]`. It is upstream 3.9.1 source plus two changes — one
behavioral (the configurable quit key) and one dependency removal (the
unmaintained `unic-emoji-char` probe).

## Status

The copy now exists for the dependency removal only. Upstream 3.10.0 ships
`App::prevent_default`, which supersedes the quit-key change: the terminal's
layered Ctrl+C moves to it when this copy is dropped (issue hawk90/revue#949).
The copy cannot be deleted yet because registry 3.10.0 still depends on
`unic-emoji-char`, whose unmaintained advisories the repository's supply-chain
policy resolves instead of acknowledging; a `cargo deny check advisories` probe
against the registry 3.10.0 graph fails with RUSTSEC-2025-0090 and
RUSTSEC-2025-0098. Upstream plans to replace that source (issue
hawk90/revue#950), and the upgrade, the `prevent_default` switch, and this
copy's deletion land together in one change.

## The patch

| File | Change |
| --- | --- |
| `src/core/app/mod.rs` | `App` gains a private `quit_key: Option<KeyEvent>` field, initialized to `Some(KeyEvent::ctrl(Key::Char('c')))` in both constructors, plus `set_quit_key`; the free `is_quit_key` predicate takes the configured key, keeps the historical Ctrl+C predicate for the default (and any other Ctrl+C binding), and matches every other configured key exactly. |
| `src/core/app/builder.rs` | `AppBuilder` gains a private `quit_key: Option<KeyEvent>` field (default Ctrl+C) and the public `quit_key(self, key: Option<KeyEvent>) -> Self` method; `build()` forwards it to `App::set_quit_key`. |
| `src/core/app/event_loop.rs` | The quit branch consults `self.quit_key` instead of the hardcoded `is_quit_key(&key)` call. |
| `Cargo.toml`, `src/text/width.rs` | The `unic-emoji-char` dependency and its single call site are removed (see below). |

`AppBuilder::quit_key(None)` disables the built-in quit key, so the app handler
sees the key and can implement its own semantics; `Some(key)` makes exactly that
chord quit. With no builder call the behavior is unchanged: Ctrl+C quits. The
upstream unit tests are updated for the predicate's new signature and gain
`test_is_quit_key_is_configurable` and `test_builder_quit_key`. All of this
gives way to `App::prevent_default` on the upgrade.

## Removed dependency: `unic-emoji-char`

Upstream probes `unic_emoji_char::is_emoji_presentation(ch)` inside
`CharWidthTable::width`, a public utility that the widgets do not use (they
measure text with a different function), to return the configurable `emoji`
width for characters drawn as emoji by default. Every `unic-*` crate carries an
unmaintained advisory (RUSTSEC-2025-0075, 0080, 0081, 0090, 0098), the
repository's supply-chain policy forbids acknowledging advisories instead of
resolving them (`quality/check_deny_policy.py`), and the crate is only reachable
through this one call site, so this copy removes the dependency and the branch.

Rendered widget behavior is unchanged. `CharWidthTable::with_emoji` loses the
probe: `unicode-width` reports the same two columns for 940 of the 966
emoji-presentation code points, while the 26 regional-indicator letters
(U+1F1E6..U+1F1FF) fall back to one column, and `with_emoji()` then applies no
override of its own. This workspace never overrides the emoji width. Upstream
plans to replace the probe with a maintained source and to keep the check
working (issue hawk90/revue#950).

## Vendor trimming

Kept: `Cargo.toml`, `build.rs`, `src/**`, `LICENSE`, `README.md`.

Dropped: `tests/`, `benches/`, `examples/`, `docs/`, `templates/`,
`tree-sitter-revue-css/`, `Cargo.lock`, `Cargo.toml.orig`, `.cargo_vcs_info.json`.
The manifest drops the matching `[[example]]`, `[[test]]`, and `[[bench]]`
targets, the `[profile.*]` tables (a dependency's profiles are ignored), and the
`criterion`, `insta`, `pretty_assertions`, `proptest`, `tokio-test`, and Windows
`windows-sys` dev-dependencies. The `serial_test` and `tempfile` dev-dependencies
stay so the retained in-source tests still build. `[package.metadata.cargo-machete]`
records the three upstream dependencies that no retained source references.

## Dropping the vendored copy

When a revue release replaces the `unic-emoji-char` source:

1. upgrade the `revue` requirement in `crates/intention-tui/Cargo.toml` to that
   release (3.10.0 or newer);
2. move the terminal's layered Ctrl+C from `AppBuilder::quit_key` to upstream's
   `App::prevent_default` in `src/tui/mod.rs::handle_event`;
3. delete `vendor/revue/`, the root `[patch.crates-io]` entry, and the
   `exclude = ["vendor/revue"]` workspace entry;
4. regenerate `Cargo.lock` and `THIRD_PARTY_NOTICES.md`;
5. remove the vendored-dependency paragraph from
   `docs/intention-relay/architecture/12-quality-gates-and-makefile.md`.
