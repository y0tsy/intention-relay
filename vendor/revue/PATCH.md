# Patched revue 3.9.1

This directory is a vendored copy of the `revue` 3.9.1 crate
(<https://github.com/hawk90/revue>, MIT) selected by the root `Cargo.toml`
through `[patch.crates-io]`. It is upstream 3.9.1 source plus two changes — one
behavioral (the configurable quit key) and one dependency removal (the
unmaintained `unic-emoji-char` probe) — kept here until upstream offers the same
option.

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
`test_is_quit_key_is_configurable` and `test_builder_quit_key`.

## Removed dependency: `unic-emoji-char`

Upstream 3.9.1 probes `unic_emoji_char::is_emoji_presentation(ch)` inside
`WidthConfig::width` to return the configurable `emoji` width for characters
drawn as emoji by default. Every `unic-*` crate carries an unmaintained advisory
(RUSTSEC-2025-0075, 0080, 0081, 0090, 0098), the repository's supply-chain policy
forbids acknowledging advisories instead of resolving them
(`quality/check_deny_policy.py`), and the crate is only reachable through this
one call site, so this copy removes the dependency and the branch.

Rendered behavior is unchanged for the default configuration: `unicode-width`
already reports two columns for an emoji-presentation character, which is the
`emoji` width produced by `WidthConfig::default()` and by `for_terminal` for the
terminals revue describes. A caller that overrides `emoji` to a width different
from `cjk` would see the base-width path instead; this workspace never does.

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

When upstream exposes a quit-key option:

1. upgrade the `revue` requirement in `crates/intention-tui/Cargo.toml`;
2. delete `vendor/revue/`, the root `[patch.crates-io]` entry, and the
   `exclude = ["vendor/revue"]` workspace entry;
3. regenerate `Cargo.lock` and `THIRD_PARTY_NOTICES.md`;
4. remove the vendored-dependency paragraph from
   `docs/intention-relay/architecture/12-quality-gates-and-makefile.md`.
