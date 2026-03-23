# rustc ICE: `StyledBuffer::replace` panics on narrow diagnostic width

## Summary

`rustc` 1.92.0+ panics when rendering any `unused_mut` lint where the variable name is 7+ characters, if `--diagnostic-width` is less than 10. The crash is in `StyledBuffer::replace`, which computes an inverted slice range when terminal width is too small.

- **1.92.0-1.93.0**: crash in `rustc_errors::styled_buffer::StyledBuffer::replace` (HumanEmitter)
- **1.94.0+**: crash in `annotate_snippets::renderer::styled_buffer::StyledBuffer::replace` (AnnotateSnippetEmitter)

This affects any environment where terminal width auto-detection returns 0 -- primarily WSL2 non-interactive invocations where `/dev/tty` reports size 1x1.

```
thread 'rustc' panicked at library/alloc/src/vec/mod.rs:2873:36:
slice index starts at 13 but ends at 11
```

## Reproduce

```bash
# Minimal -- one line of valid Rust:
echo 'pub fn f() { let mut foo_bar = 0; }' > ice.rs

# ICEs on 1.92.0+:
rustc +1.92.0 --edition=2021 --crate-type lib --diagnostic-width=0 ice.rs
rustc +1.94.0 --edition=2021 --crate-type lib --diagnostic-width=0 ice.rs

# OK on 1.91.0:
rustc +1.91.0 --edition=2021 --crate-type lib --diagnostic-width=0 ice.rs
```

Expected: compiles with `unused_mut` warning.
Actual: ICE.

### What triggers it

- Any `unused_mut` lint where the variable name is **7+ characters**
- `--diagnostic-width` is **less than 10** (including 0)
- That's it. No special types, no async, no `#[cfg]`, no dependencies.

The 7-character threshold comes from the annotation span width needing to exceed 10 display columns to enter the span-trimming codepath. The `unused_mut` suggestion underlines `mut aaaaaaa` = 12 columns (> 10).

### Width threshold

| `--diagnostic-width` | Result |
|---|---|
| 0-9 | ICE |
| **10+** | OK |

## Root cause

### The vulnerable code

Both `rustc_errors::StyledBuffer::replace` (used by HumanEmitter in 1.92-1.93) and `annotate_snippets::StyledBuffer::replace` (used by AnnotateSnippetEmitter in 1.94+) have the same bug. The caller trims long annotations for narrow terminals:

```rust
let width = annotation.end.display - annotation.start.display;
if width > margin.term_width * 2 && width > 10 {
    let pad = max(margin.term_width / 3, 5);
    buffer.replace(
        line_offset,
        annotation.start.display + pad,   // start
        annotation.end.display - pad,       // end
        "...",
    );
}
```

When `term_width` is 0:
- `width > 0 * 2` -- true for any non-zero span (guard is bypassed)
- `pad = max(0 / 3, 5) = 5`
- `start = annotation.start.display + 5` -- e.g. 13
- `end = annotation.end.display - 5` -- e.g. 8
- `start > end` -- inverted range

The `replace()` method has no guard on the range:

```rust
let _ = self.lines[line].drain(start..(end - string.chars().count()));
// panics: "slice index starts at 13 but ends at 11"
```

### How this was discovered

This bug was originally found in a 155-line async fn that used `#[cfg(windows)]`, `OsString`, `Send` bounds, and `&mut` parameters. It appeared to require all of those, but that was because:

1. The `#[cfg(windows)]` made a mutation dead on Linux, triggering `unused_mut`
2. The variable name (`env_vars`, 8 chars) was long enough to exceed the 10-column threshold
3. The async/OsString/Send combination was necessary for the *original* code but not for the bug itself

The actual trigger is just **any `unused_mut` warning with a 7+ char variable name at `diagnostic-width < 10`**.

## Version bisect

| Toolchain | Result | Crash location |
|-----------|--------|----------------|
| 1.91.0 | OK | -- |
| **1.92.0** | **ICE** | `rustc_errors::StyledBuffer::replace` (HumanEmitter) |
| **1.93.0** | **ICE** | `rustc_errors::StyledBuffer::replace` (HumanEmitter) |
| **1.94.0** | **ICE** | `annotate_snippets::StyledBuffer::replace` (AnnotateSnippetEmitter) |
| 1.95.0-beta.4 | ICE | annotate_snippets |
| 1.96.0-nightly | ICE | annotate_snippets |

### Nightly bisect: 1.91 -> 1.92 (`cargo-bisect-rustc`)

```
searched nightlies: from nightly-2025-08-01 to nightly-2025-10-30
regressed nightly:  nightly-2025-10-13
regressed commit:   rust-lang/rust@ff6dc928
```

The regressing commit is the auto-merge of [#142390](https://github.com/rust-lang/rust/pull/142390) ("Perform unused assignment and unused variables lints on MIR", merged Oct 12, 2025). This changed how `unused_mut` annotations are structured, causing them to hit the pre-existing bug in `StyledBuffer::replace` at small terminal widths.

### Why 1.94.0 uses a different code path

In 1.94.0, the default diagnostic emitter switched from `HumanEmitter` to `AnnotateSnippetEmitter` via [#150032](https://github.com/rust-lang/rust/pull/150032). The annotate-snippets crate has its own copy of `StyledBuffer::replace` with the same bug. On current rustc main, the old `styled_buffer.rs` has been removed entirely -- only the annotate-snippets path remains.

## Investigation: why it looked environment-specific

The bug was first observed as WSL2-specific -- it reproduced on a WSL2 host but not in Docker on the same host, nor on a native Linux VPS. This led to an extensive forensic investigation before the terminal width connection was found.

### Ruling out hypotheses

We verified **byte-identical binaries** across all environments:

| Component | SHA-256 (identical everywhere) |
|---|---|
| `rustc` | `103b60e1...836a` |
| `libc.so.6` (glibc 2.39) | `d8db8739...d75` |
| `librustc_driver.so` | `d3bc8f47...62b` |
| `libLLVM.so` | `bfa77bb3...4b` |
| `ld-linux-x86-64.so.2` | `1cd555ac...829` |

All of these produced different behavior on the host vs Docker. We systematically ruled out:

| Hypothesis | Test | Result |
|---|---|---|
| WSL2 kernel | Docker on same WSL2 kernel | OK in Docker |
| glibc version | Ubuntu 24.04 Docker (same glibc 2.39) | OK in Docker |
| Threading | `-j1`, `-Zthreads=1` | Still ICEs on host |
| jemalloc arenas | `MALLOC_CONF="narenas:1"` | Still ICEs on host |
| ASLR | `setarch x86_64 -R` | Still ICEs on host |
| Environment variables | `env -i HOME=/tmp PATH=... rustc ...` | Still ICEs on host |
| Filesystem (9P vs ext4) | Tested from `/tmp` (ext4) | Still ICEs on host |
| TERM / locale | `TERM=`, `TERM=dumb` | Still ICEs on host |
| Docker namespaces | `--privileged --pid=host --ipc=host --net=host` | Still OK in Docker |
| cgroup CPU limits | Both show `cpu.max = max 100000`, 16 CPUs | Same |
| `/proc/self/cgroup` path | Host: `/init.scope`, Docker: `/` | No effect |

### The breakthrough

After exhausting environmental hypotheses, we discovered that **suppressing `unused_mut`** with `-A unused-mut` eliminated the crash entirely. This pointed to the diagnostic renderer, not the type system.

We then checked terminal width detection:

```bash
$ stty size < /dev/tty  # on WSL2 host (non-interactive)
1 1
```

Non-interactive WSL2 invocations (from Windows `wsl -e`) report `/dev/tty` size as 1x1. rustc detects this and sets `diagnostic-width=0`, triggering the bug.

| Environment | `/dev/tty` | Auto-detected width | Result |
|---|---|---|---|
| WSL2 host (non-interactive) | 1x1 | 0 | ICE |
| Docker (no `/dev/tty`) | n/a | default (~140) | OK |
| SSH to Linux VPS | real terminal | 80+ | OK |
| Any Linux + `--diagnostic-width=0` | irrelevant | forced 0 | ICE |

## Backtraces

<details>
<summary>1.94.0 (AnnotateSnippetEmitter)</summary>

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
```

</details>

<details>
<summary>1.92.0 (HumanEmitter)</summary>

```
12: core::slice::index::slice_index_fail
13: core::slice::index::slice_index_fail
14: <rustc_errors::styled_buffer::StyledBuffer>::replace
15: <rustc_errors::emitter::HumanEmitter>::emit_messages_default_inner::{closure#0}
16: <rustc_errors::emitter::HumanEmitter as rustc_errors::emitter::Emitter>::emit_diagnostic
```

</details>

## Fix status

- **annotate-snippets (affects 1.94.0+)**: [PR #392](https://github.com/rust-lang/annotate-snippets-rs/pull/392) -- guards inverted ranges in `StyledBuffer::replace` and skips trimming when `term_width == 0`
- **rustc_errors (affects 1.92.0-1.93.0)**: `styled_buffer.rs` was removed from rustc main when the emitter switched to annotate-snippets. A backport would need to patch the old `StyledBuffer::replace` with the same fix.

## Tracked issues

- rustc: https://github.com/rust-lang/rust/issues/154258
- annotate-snippets: https://github.com/rust-lang/annotate-snippets-rs/issues/391

## Platform info

```
rustc 1.94.0 (4a4ef493e 2026-03-02) on x86_64-unknown-linux-gnu
annotate-snippets 0.12.10 (bundled in rustc)
Tested: Ubuntu 24.04 (WSL2 + Docker + native VPS), glibc 2.39
```

## Workarounds

```bash
# Set an explicit terminal width
RUSTFLAGS="--diagnostic-width=80" cargo build

# Suppress the triggering lint
RUSTFLAGS="-A unused-mut" cargo build

# Pin to the last working toolchain
rustup override set 1.91.0
```
