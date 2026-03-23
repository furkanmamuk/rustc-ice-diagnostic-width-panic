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
- `--diagnostic-width` is less than 10 (including 0, the auto-detected value when there is no TTY)

This is hit in practice by non-interactive WSL2 invocations (`wsl -e bash -c ...`), where `/dev/tty` reports size 1x1 and rustc auto-detects width as 0.

The crash also produces a secondary `delayed_bug` about `OpaqueTypeKey` / `ProvisionalHiddenType`, but this is collateral damage from the panic aborting the borrow checker -- not an independent type system bug.

## Regression

Regression in **1.92.0** (works on 1.91.0). Two bisections:

**Bisect 1: 1.91.0 -> 1.92.0 (the actual regression)**

```
searched nightlies: from nightly-2025-08-01 to nightly-2025-10-30
regressed nightly: nightly-2025-10-13
regressed commit: rust-lang/rust@ff6dc928
```

The regressing commit is the auto-merge of [#142390](https://github.com/rust-lang/rust/pull/142390) ("Perform unused assignment and unused variables lints on MIR"), which changed how `unused_mut` annotations are structured. This caused them to hit a pre-existing bug in `StyledBuffer::replace` at small terminal widths.

**Bisect 2: emitter switch in 1.94.0**

In 1.94.0, the crash moved from `rustc_errors::StyledBuffer::replace` (HumanEmitter) to `annotate_snippets::StyledBuffer::replace` (AnnotateSnippetEmitter) due to the emitter switch in [#150032](https://github.com/rust-lang/rust/pull/150032). Both `StyledBuffer::replace` implementations have the same bug.

| Version | ICE? | Crash location |
|---------|------|----------------|
| 1.91.0 | No | -- |
| **1.92.0** | **Yes** | `rustc_errors::StyledBuffer::replace` (HumanEmitter) |
| **1.93.0** | **Yes** | `rustc_errors::StyledBuffer::replace` (HumanEmitter) |
| **1.94.0** | **Yes** | `annotate_snippets::StyledBuffer::replace` (AnnotateSnippetEmitter) |

## Root cause

The bug is in `StyledBuffer::replace` -- it doesn't guard against inverted ranges. The span-trimming logic computes `start + pad` and `end - pad`, which produces `start > end` when `term_width` is 0 or very small. The `replace()` method then panics on the inverted drain range.

On current rustc main, the old `rustc_errors::styled_buffer.rs` has been removed (emitter fully switched to annotate-snippets), so the fix only needs to land in annotate-snippets.

Filed upstream: https://github.com/rust-lang/annotate-snippets-rs/issues/391
Fix PR: https://github.com/rust-lang/annotate-snippets-rs/pull/392

## Workarounds

```bash
RUSTFLAGS="--diagnostic-width=80" cargo build   # explicit width
RUSTFLAGS="-A unused-mut" cargo build            # suppress the lint
rustup override set 1.91.0                       # pin toolchain
```

## Reproducer repo

https://github.com/furkanmamuk/rustc-ice-diagnostic-width-panic
