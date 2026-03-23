## Code

```rust
pub fn f() { let mut foo_bar = 0; }
```

## Meta

`rustc --version --verbose`:
```
rustc 1.94.0 (4a4ef493e 2026-03-02)
binary: rustc
commit-hash: 4a4ef493e3a1488c6e321570238084b38948f6db
commit-date: 2026-03-02
host: x86_64-unknown-linux-gnu
release: 1.94.0
LLVM version: 21.1.8
```

## Error output

```
$ rustc --edition=2021 --crate-type lib --diagnostic-width=0 ice.rs

thread 'rustc' panicked at library/alloc/src/vec/mod.rs:2873:36:
slice index starts at 13 but ends at 11
...
  12: <annotate_snippets::renderer::styled_buffer::StyledBuffer>::replace
  13: annotate_snippets::renderer::render::render
  14: <rustc_errors::annotate_snippet_emitter_writer::AnnotateSnippetEmitter>::emit_messages_default
...
  21: rustc_middle::lint::lint_level::lint_level_impl
  22: rustc_borrowck::borrowck_check_region_constraints

note: we would appreciate a bug report: https://github.com/rust-lang/rust/issues/new
note: rustc 1.94.0 (4a4ef493e 2026-03-02) running on x86_64-unknown-linux-gnu
```

<details>
<summary>Full backtrace</summary>

```
 0: <std::sys::backtrace::BacktraceLock::print::DisplayBacktrace as core::fmt::Display>::fmt
 1: core::fmt::write
 2: std::io::Write::write_fmt
 3: std::panicking::default_hook::{closure}
 4: std::panicking::default_hook
 5: std::panicking::update_hook::<Box<rustc_driver_impl::install_ice_hook::{closure#1}>>::{closure#0}
 6: std::panicking::panic_with_hook
 7: std::panicking::panic_handler::{closure}
 8: std::sys::backtrace::__rust_end_short_backtrace
 9: __rustc::rust_begin_unwind
10: core::panicking::panic_fmt
11: core::slice::index::slice_index_fail
12: <annotate_snippets::renderer::styled_buffer::StyledBuffer>::replace
13: annotate_snippets::renderer::render::render
14: <rustc_errors::annotate_snippet_emitter_writer::AnnotateSnippetEmitter>::emit_messages_default
15: <AnnotateSnippetEmitter as rustc_errors::emitter::Emitter>::emit_diagnostic
16: <rustc_errors::DiagCtxtInner>::emit_diagnostic::{closure#3}
17: rustc_interface::callbacks::track_diagnostic::<Option<ErrorGuaranteed>>
18: <rustc_errors::DiagCtxtInner>::emit_diagnostic
19: <rustc_errors::DiagCtxtHandle>::emit_diagnostic
20: <() as rustc_errors::diagnostic::EmissionGuarantee>::emit_producing_guarantee
21: rustc_middle::lint::lint_level::lint_level_impl
22: rustc_borrowck::borrowck_check_region_constraints
23: <rustc_borrowck::root_cx::BorrowCheckRootCtxt>::do_mir_borrowck
```

</details>

## Trigger conditions

- Any `unused_mut` warning where the variable name is 7+ characters (the lint's underline spans 12+ display columns)
- `--diagnostic-width` is 10 or less (including 0, the auto-detected value when there is no TTY)

This is hit in practice by non-interactive environments: CI without a PTY, `wsl -e` invocations, piped output through tools that don't allocate a terminal, etc.

The crash also produces a secondary `delayed_bug` about `OpaqueTypeKey` / `ProvisionalHiddenType`, but this is collateral damage from the panic aborting the borrow checker -- not an independent type system bug.

## Regression

Regression in **1.94.0** (works on 1.93.0). Nightly bisect points to the annotate-snippets 0.12.x upgrade: [#148984](https://github.com/rust-lang/rust/pull/148984) (0.12.9, merged Nov 16) and [#149529](https://github.com/rust-lang/rust/pull/149529) (0.12.10, merged Dec 2). The `StyledBuffer::replace` method was introduced in 0.12.x.

```
searched nightlies: nightly-2025-11-01 to nightly-2026-01-20
regressed nightly:  nightly-2025-11-23
```

## Root cause

The bug is in `annotate-snippets` 0.12.10-0.12.13 (still present on main), not in rustc itself. The span-trimming logic in `annotate_snippets::renderer::render` computes an inverted range when `term_width` is 0, and `StyledBuffer::replace` panics on it.

Filed upstream: https://github.com/rust-lang/annotate-snippets-rs/issues/391

## Workarounds

```bash
RUSTFLAGS="--diagnostic-width=80" cargo build   # explicit width
RUSTFLAGS="-A unused-mut" cargo build            # suppress the lint
rustup override set 1.93.0                       # pin toolchain
```

## Reproducer repo

https://github.com/furkanmamuk/rustc-ice-diagnostic-width-panic
