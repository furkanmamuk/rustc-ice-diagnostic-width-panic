# `StyledBuffer::replace` panics when `term_width` is 0 or very small

## Summary

`StyledBuffer::replace` panics with "slice index starts at 13 but ends at 11" when `Renderer::term_width` is set to 10 or less and an annotation's display width exceeds 10 columns.

This causes an ICE in `rustc` 1.94.0+ (which bundles `annotate-snippets` 0.12.10) for any `unused_mut` warning with a 7+ character variable name when `--diagnostic-width=0`.

## Reproduce

```rust
use annotate_snippets::{AnnotationKind, Level, Renderer, Snippet};

fn main() {
    let source = "pub fn f() { let mut foo_bar = 0; }";
    let input = &[Level::WARNING
        .primary_title("variable does not need to be mutable")
        .element(
            Snippet::source(source).path("ice.rs").annotation(
                AnnotationKind::Primary
                    .span(17..29)
                    .label("help: remove this `mut`"),
            ),
        )];

    let renderer = Renderer::plain().term_width(0);
    let _ = renderer.render(input); // panics
}
```

Also reproducible via rustc directly:

```bash
echo 'pub fn f() { let mut foo_bar = 0; }' > ice.rs
rustc +1.94.0 --edition=2021 --crate-type lib --diagnostic-width=0 ice.rs
```

## Root cause

The span-trimming logic in `src/renderer/render.rs:1395`:

```rust
if width > margin.term_width * 2 && width > 10 {
    let pad = max(margin.term_width / 3, 5);
    buffer.replace(
        line_offset,
        annotation.start.display + pad,   // start
        annotation.end.display - pad,       // end -- can be < start
        renderer.decor_style.margin(),
    );
}
```

When `term_width` is 0:
- `width > 0 * 2` is true for any span (the guard is useless)
- `pad = max(0 / 3, 5) = 5`
- For a 12-column annotation: `start = 8 + 5 = 13`, `end = 20 - 5 = 15`
- In `replace()`: `drain(13..(15 - 3)) = drain(13..12)` -- panics

The `replace` method itself also lacks bounds checking for inverted ranges.

## Affected versions

- 0.12.10 through 0.12.13 (current main) -- all have the same code
- `replace()` was introduced in the 0.12.x series; 0.11.x is not affected

## Reproducer repo

https://github.com/furkanmamuk/rustc-ice-diagnostic-width-panic
