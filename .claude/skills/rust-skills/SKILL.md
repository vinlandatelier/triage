---
name: rust-skills
description: >
  Comprehensive Rust coding guidelines with 153 rules across 12 categories.
  Use when writing, reviewing, or refactoring Rust code. Covers error
  handling, type safety, API design, async patterns, project structure, naming,
  testing, observability, serde, linting, pattern matching, and common
  anti-patterns.
  Invoke with /rust-skills.
license: MIT
metadata:
  author: leonardomso
  version: "1.5.1"
  sources:
    - Rust API Guidelines
    - Rust Performance Book
    - Rust 2024 Edition Guide
    - The Rustonomicon
    - ripgrep, tokio, serde, polars, axum, cargo codebases
---

# Rust Best Practices

Comprehensive guide for writing high-quality, idiomatic, and highly optimized Rust code. Contains 153 rules across 12 categories, prioritized by impact to guide LLMs in code generation and refactoring. Current for Rust 1.96 (2024 edition).

## When to Apply

Reference these guidelines when:
- Writing new Rust functions, structs, or modules
- Implementing error handling or async code
- Writing concurrent, parallel, or `unsafe` code
- Designing public APIs for libraries
- Reviewing code for ownership/borrowing issues
- Optimizing memory usage or reducing allocations
- Tuning performance for hot paths
- Refactoring existing Rust code

## Rule Categories by Priority

| Priority | Category | Impact | Prefix | Rules |
|----------|----------|--------|--------|-------|
| 1 | Error Handling | CRITICAL | `err-` | 12 |
| 2 | API Design | HIGH | `api-` | 17 |
| 3 | Async/Await | HIGH | `async-` | 18 |
| 4 | Type Safety | MEDIUM | `type-` | 13 |
| 5 | Serde | MEDIUM | `serde-` | 8 |
| 6 | Pattern Matching | MEDIUM | `pat-` | 5 |
| 7 | Naming Conventions | MEDIUM | `name-` | 16 |
| 8 | Testing | MEDIUM | `test-` | 15 |
| 9 | Observability | MEDIUM | `obs-` | 7 |
| 10 | Project Structure | LOW | `proj-` | 14 |
| 11 | Clippy & Linting | LOW | `lint-` | 13 |
| 12 | Anti-patterns | REFERENCE | `anti-` | 15 |

---

## Quick Reference

### 1. Error Handling (CRITICAL)

- [`err-thiserror-lib`](rules/err-thiserror-lib.md) - Use `thiserror` for library error types
- [`err-anyhow-app`](rules/err-anyhow-app.md) - Use `anyhow` only in `main.rs`, `cli.rs`, `build.rs`, and as the `AppError::Internal` payload
- [`err-result-over-panic`](rules/err-result-over-panic.md) - Return `Result<T, E>` instead of panicking for recoverable errors
- [`err-context-chain`](rules/err-context-chain.md) - Add context with `.context()` or `.with_context()`
- [`err-no-unwrap-prod`](rules/err-no-unwrap-prod.md) - No `unwrap()` or `expect()` outside tests; use `?` or handle the error
- [`err-expect-bugs-only`](rules/err-expect-bugs-only.md) - No `expect()` outside tests; a broken invariant returns `AppError::Internal`
- [`err-question-mark`](rules/err-question-mark.md) - Use `?` operator for clean propagation
- [`err-from-impl`](rules/err-from-impl.md) - Implement `From<E>` for error conversions to enable `?` operator
- [`err-source-chain`](rules/err-source-chain.md) - Preserve error chains with `#[source]` or `source()` method
- [`err-lowercase-msg`](rules/err-lowercase-msg.md) - Start error messages lowercase, no trailing punctuation
- [`err-doc-errors`](rules/err-doc-errors.md) - Document error conditions with `# Errors` section in doc comments
- [`err-custom-type`](rules/err-custom-type.md) - Define custom error types for domain-specific failures

### 2. API Design (HIGH)

- [`api-builder-pattern`](rules/api-builder-pattern.md) - Use Builder pattern for complex construction
- [`api-builder-must-use`](rules/api-builder-must-use.md) - Mark builder methods with `#[must_use]` to prevent silent drops
- [`api-newtype-safety`](rules/api-newtype-safety.md) - Use newtypes to prevent mixing semantically different values
- [`api-typestate`](rules/api-typestate.md) - Use typestate pattern to encode state machine invariants in the type system
- [`api-sealed-trait`](rules/api-sealed-trait.md) - Use sealed traits to prevent external implementations while allowing use
- [`api-extension-trait`](rules/api-extension-trait.md) - Use extension traits to add methods to external types
- [`api-parse-dont-validate`](rules/api-parse-dont-validate.md) - Parse into validated types at boundaries
- [`api-impl-into`](rules/api-impl-into.md) - Accept `impl Into<T>` for flexible APIs, implement `From<T>` for conversions
- [`api-impl-asref`](rules/api-impl-asref.md) - Use `AsRef<T>` when you only need to borrow the inner data
- [`api-must-use`](rules/api-must-use.md) - Mark types and functions with `#[must_use]` when ignoring results is likely a bug
- [`api-non-exhaustive`](rules/api-non-exhaustive.md) - Use `#[non_exhaustive]` on public enums and structs for forward compatibility
- [`api-from-not-into`](rules/api-from-not-into.md) - Implement `From<T>`, not `Into<U>` - From gives you Into for free
- [`api-default-impl`](rules/api-default-impl.md) - Implement `Default` for types with sensible default values
- [`api-common-traits`](rules/api-common-traits.md) - Implement standard traits (Debug, Clone, PartialEq, etc.) for public types
- [`api-serde-optional`](rules/api-serde-optional.md) - Make serde a feature flag, not a hard dependency for library crates
- [`api-impl-fromiterator`](rules/api-impl-fromiterator.md) - Implement `FromIterator` and `Extend` for collection types, and `IntoIterator` for all three reference forms
- [`api-operator-overload`](rules/api-operator-overload.md) - Overload operators only when the semantics are natural and unsurprising

### 3. Async/Await (HIGH)

- [`async-tokio-runtime`](rules/async-tokio-runtime.md) - Configure Tokio runtime appropriately for your workload
- [`async-no-lock-await`](rules/async-no-lock-await.md) - Never hold `Mutex`/`RwLock` across `.await`
- [`async-spawn-blocking`](rules/async-spawn-blocking.md) - Use `spawn_blocking` for CPU-intensive work
- [`async-tokio-fs`](rules/async-tokio-fs.md) - Use `tokio::fs` instead of `std::fs` in async code
- [`async-cancellation-token`](rules/async-cancellation-token.md) - Use `CancellationToken` for graceful shutdown and task cancellation
- [`async-join-parallel`](rules/async-join-parallel.md) - Use `join!` or `try_join!` for concurrent independent futures
- [`async-try-join`](rules/async-try-join.md) - Use `try_join!` for concurrent fallible operations with early return on error
- [`async-select-racing`](rules/async-select-racing.md) - Use `select!` to race futures and handle the first to complete
- [`async-bounded-channel`](rules/async-bounded-channel.md) - Use bounded channels to apply backpressure and prevent unbounded memory growth
- [`async-mpsc-queue`](rules/async-mpsc-queue.md) - Use `mpsc` channels for async message queues between tasks
- [`async-broadcast-pubsub`](rules/async-broadcast-pubsub.md) - Use `broadcast` channel for pub/sub where all subscribers receive all messages
- [`async-watch-latest`](rules/async-watch-latest.md) - Use `watch` channel for sharing the latest value with multiple observers
- [`async-oneshot-response`](rules/async-oneshot-response.md) - Use `oneshot` channel for request-response patterns
- [`async-joinset-structured`](rules/async-joinset-structured.md) - Use `JoinSet` for managing dynamic collections of spawned tasks
- [`async-clone-before-await`](rules/async-clone-before-await.md) - Clone Arc/Rc data before await points to avoid holding references across suspension
- [`async-fn-in-trait`](rules/async-fn-in-trait.md) - Use native `async fn` in traits (stable 1.75) instead of the `async_trait` macro
- [`async-async-fn-bounds`](rules/async-async-fn-bounds.md) - Use `AsyncFn`/`AsyncFnMut`/`AsyncFnOnce` bounds instead of `F: Fn() -> Fut, Fut: Future`
- [`async-cancel-safety`](rules/async-cancel-safety.md) - Ensure futures used in `tokio::select!` branches are cancellation-safe

### 4. Type Safety (MEDIUM)

- [`type-newtype-ids`](rules/type-newtype-ids.md) - Wrap IDs in newtypes: `UserId(u64)`
- [`type-newtype-validated`](rules/type-newtype-validated.md) - Use newtypes to enforce validation at construction time
- [`type-enum-states`](rules/type-enum-states.md) - Use enums for mutually exclusive states
- [`type-option-nullable`](rules/type-option-nullable.md) - Use `Option<T>` for values that might not exist
- [`type-result-fallible`](rules/type-result-fallible.md) - Use `Result<T, E>` for operations that can fail
- [`type-phantom-marker`](rules/type-phantom-marker.md) - Use `PhantomData` to express type relationships without runtime cost
- [`type-never-diverge`](rules/type-never-diverge.md) - Use `!` (never type) for functions that never return
- [`type-generic-bounds`](rules/type-generic-bounds.md) - Add trait bounds only where needed, prefer where clauses for readability
- [`type-no-stringly`](rules/type-no-stringly.md) - Avoid stringly-typed APIs; use enums, newtypes, or validated types
- [`type-repr-transparent`](rules/type-repr-transparent.md) - Use `#[repr(transparent)]` for newtypes in FFI contexts
- [`type-deref-coercion`](rules/type-deref-coercion.md) - Implement `Deref`/`DerefMut` only for smart-pointer and transparent wrapper types
- [`type-display-vs-debug`](rules/type-display-vs-debug.md) - Use `Display` for user-facing output and `Debug` for diagnostics; never swap them
- [`type-numeric-fmt`](rules/type-numeric-fmt.md) - Implement `LowerHex`, `UpperHex`, `Octal`, and `Binary` for numeric newtypes

### 5. Serde (MEDIUM)

- [`serde-rename-all`](rules/serde-rename-all.md) - Match the external naming convention with `#[serde(rename_all = ...)]`
- [`serde-default-compat`](rules/serde-default-compat.md) - Use `#[serde(default)]` for optional and backward-compatible fields
- [`serde-skip-empty`](rules/serde-skip-empty.md) - Omit empty fields with `skip_serializing_if`
- [`serde-flatten`](rules/serde-flatten.md) - Inline nested structs or capture extra keys with `#[serde(flatten)]`
- [`serde-enum-representation`](rules/serde-enum-representation.md) - Choose enum tagging deliberately: externally, internally, adjacently tagged, or untagged
- [`serde-deny-unknown-fields`](rules/serde-deny-unknown-fields.md) - Reject unexpected keys with `#[serde(deny_unknown_fields)]`
- [`serde-custom-with`](rules/serde-custom-with.md) - Customize a field's (de)serialization with `with` / `serialize_with` / `deserialize_with`
- [`serde-try-from-validate`](rules/serde-try-from-validate.md) - Validate while deserializing with `#[serde(try_from = "Raw")]`

### 6. Pattern Matching (MEDIUM)

- [`pat-let-else`](rules/pat-let-else.md) - Use `let ... else` for early-return pattern extraction
- [`pat-matches-macro`](rules/pat-matches-macro.md) - Use `matches!()` for boolean pattern tests
- [`pat-if-let-chains`](rules/pat-if-let-chains.md) - Use `if let` chains to combine pattern bindings and conditions
- [`pat-exhaustive-enum`](rules/pat-exhaustive-enum.md) - Match owned enums exhaustively; avoid catch-all `_` that hides new variants
- [`pat-at-bindings`](rules/pat-at-bindings.md) - Use `@` bindings to capture a value while matching it against a pattern

### 7. Naming Conventions (MEDIUM)

- [`name-types-camel`](rules/name-types-camel.md) - Use `UpperCamelCase` for types, traits, and enum names
- [`name-variants-camel`](rules/name-variants-camel.md) - Use `UpperCamelCase` for enum variants
- [`name-funcs-snake`](rules/name-funcs-snake.md) - Use `snake_case` for functions, methods, variables, and modules
- [`name-consts-screaming`](rules/name-consts-screaming.md) - Use `SCREAMING_SNAKE_CASE` for constants and statics
- [`name-lifetime-short`](rules/name-lifetime-short.md) - Use short, conventional lifetime names: `'a`, `'b`, `'de`, `'src`
- [`name-type-param-single`](rules/name-type-param-single.md) - Use single uppercase letters for type parameters: `T`, `E`, `K`, `V`
- [`name-as-free`](rules/name-as-free.md) - `as_` prefix: free reference conversion
- [`name-to-expensive`](rules/name-to-expensive.md) - Use `to_` prefix for expensive conversions that allocate or compute
- [`name-into-ownership`](rules/name-into-ownership.md) - Use `into_` prefix for ownership-consuming conversions
- [`name-no-get-prefix`](rules/name-no-get-prefix.md) - Omit get_ prefix for simple getters
- [`name-is-has-bool`](rules/name-is-has-bool.md) - Use `is_`, `has_`, `can_`, `should_` prefixes for boolean-returning methods
- [`name-iter-convention`](rules/name-iter-convention.md) - Use iter/iter_mut/into_iter for iterator methods
- [`name-iter-method`](rules/name-iter-method.md) - Name iterator methods `iter()`, `iter_mut()`, and `into_iter()` consistently
- [`name-iter-type-match`](rules/name-iter-type-match.md) - Name iterator types after their source method
- [`name-acronym-word`](rules/name-acronym-word.md) - Treat acronyms as words in identifiers: `HttpServer`, not `HTTPServer`
- [`name-crate-no-rs`](rules/name-crate-no-rs.md) - Don't suffix crate names with `-rs` or `-rust`

### 8. Testing (MEDIUM)

- [`test-cfg-test-module`](rules/test-cfg-test-module.md) - Put unit tests in `#[cfg(test)] mod tests { }` within each module
- [`test-use-super`](rules/test-use-super.md) - Use `use super::*;` in test modules to access parent module items
- [`test-integration-dir`](rules/test-integration-dir.md) - Put integration tests in the `tests/` directory
- [`test-descriptive-names`](rules/test-descriptive-names.md) - Use descriptive test names that explain what is being tested
- [`test-arrange-act-assert`](rules/test-arrange-act-assert.md) - Structure tests with clear Arrange, Act, Assert sections
- [`test-proptest-properties`](rules/test-proptest-properties.md) - Use proptest for property-based testing
- [`test-mockall-mocking`](rules/test-mockall-mocking.md) - Use mockall for trait mocking
- [`test-mock-traits`](rules/test-mock-traits.md) - Use traits for dependencies to enable mocking in tests
- [`test-fixture-raii`](rules/test-fixture-raii.md) - Use RAII pattern (Drop trait) for automatic test cleanup
- [`test-tokio-async`](rules/test-tokio-async.md) - Use `#[tokio::test]` for async tests
- [`test-should-panic`](rules/test-should-panic.md) - Use `#[should_panic]` to test that code panics as expected
- [`test-criterion-bench`](rules/test-criterion-bench.md) - Use `criterion` for benchmarking
- [`test-doctest-examples`](rules/test-doctest-examples.md) - Keep documentation examples as executable doctests
- [`test-loom-concurrency`](rules/test-loom-concurrency.md) - Use `loom` to exhaustively test lock-free and concurrent code
- [`test-snapshot-testing`](rules/test-snapshot-testing.md) - Use snapshot testing (insta) for complex or serialized output

### 9. Observability (MEDIUM)

- [`obs-tracing-over-log`](rules/obs-tracing-over-log.md) - Use `tracing` for structured, span-aware diagnostics instead of `println!` or bare `log`
- [`obs-library-facade`](rules/obs-library-facade.md) - Libraries emit through the tracing/log facade and never install a subscriber
- [`obs-structured-fields`](rules/obs-structured-fields.md) - Record structured key-value fields, not values interpolated into the message string
- [`obs-instrument-spans`](rules/obs-instrument-spans.md) - Use `#[tracing::instrument]` and spans to attach context to async tasks and requests
- [`obs-levels-filter`](rules/obs-levels-filter.md) - Use log levels meaningfully and filter with `EnvFilter` / `RUST_LOG`
- [`obs-error-chain`](rules/obs-error-chain.md) - Log errors with their full source chain, and log each error exactly once
- [`obs-no-sensitive-data`](rules/obs-no-sensitive-data.md) - Never log secrets or PII; redact or skip them

### 10. Project Structure (LOW)

- [`proj-lib-main-split`](rules/proj-lib-main-split.md) - Keep `main.rs` minimal, logic in `lib.rs`
- [`proj-mod-by-feature`](rules/proj-mod-by-feature.md) - Organize modules by feature, not type
- [`proj-flat-small`](rules/proj-flat-small.md) - Keep small projects flat
- [`proj-mod-rs-dir`](rules/proj-mod-rs-dir.md) - Use mod.rs for multi-file modules
- [`proj-pub-crate-internal`](rules/proj-pub-crate-internal.md) - Use pub(crate) for internal APIs
- [`proj-pub-super-parent`](rules/proj-pub-super-parent.md) - Use pub(super) for parent-only visibility
- [`proj-pub-use-reexport`](rules/proj-pub-use-reexport.md) - Use pub use for clean public API
- [`proj-prelude-module`](rules/proj-prelude-module.md) - Create prelude module for common imports
- [`proj-bin-dir`](rules/proj-bin-dir.md) - Put multiple binaries in src/bin/
- [`proj-workspace-large`](rules/proj-workspace-large.md) - Use workspaces for large projects
- [`proj-workspace-deps`](rules/proj-workspace-deps.md) - Use workspace dependency inheritance for consistent versions across crates
- [`proj-feature-additive`](rules/proj-feature-additive.md) - Design Cargo features to be strictly additive
- [`proj-msrv-declare`](rules/proj-msrv-declare.md) - Declare `rust-version` (MSRV) in Cargo.toml and test it in CI
- [`proj-build-rs-minimal`](rules/proj-build-rs-minimal.md) - Keep `build.rs` minimal, deterministic, and idempotent

### 11. Clippy & Linting (LOW)

- [`lint-deny-correctness`](rules/lint-deny-correctness.md) - `#![deny(clippy::correctness)]`
- [`lint-warn-suspicious`](rules/lint-warn-suspicious.md) - Enable clippy::suspicious for likely bugs
- [`lint-warn-style`](rules/lint-warn-style.md) - Enable clippy::style for idiomatic code
- [`lint-warn-complexity`](rules/lint-warn-complexity.md) - Enable clippy::complexity for simpler code
- [`lint-warn-perf`](rules/lint-warn-perf.md) - Enable clippy::perf for performance improvements
- [`lint-pedantic-selective`](rules/lint-pedantic-selective.md) - Enable clippy::pedantic selectively
- [`lint-missing-docs`](rules/lint-missing-docs.md) - Warn on missing documentation for public items
- [`lint-unsafe-doc`](rules/lint-unsafe-doc.md) - Require documentation for unsafe blocks
- [`lint-cargo-metadata`](rules/lint-cargo-metadata.md) - Enable clippy::cargo for published crates
- [`lint-rustfmt-check`](rules/lint-rustfmt-check.md) - Run cargo fmt --check in CI
- [`lint-workspace-lints`](rules/lint-workspace-lints.md) - Configure lints at workspace level for consistent enforcement
- [`lint-cfg-check`](rules/lint-cfg-check.md) - Enable `unexpected_cfgs` and declare known cfgs to catch feature-gate typos
- [`lint-clippy-nursery-selected`](rules/lint-clippy-nursery-selected.md) - Enable high-value `clippy::nursery` lints selectively, not the whole group

### 12. Anti-patterns (REFERENCE)

- [`anti-unwrap-abuse`](rules/anti-unwrap-abuse.md) - Don't use `.unwrap()` in production code
- [`anti-expect-lazy`](rules/anti-expect-lazy.md) - Don't use expect for recoverable errors
- [`anti-clone-excessive`](rules/anti-clone-excessive.md) - Don't clone when borrowing works
- [`anti-lock-across-await`](rules/anti-lock-across-await.md) - Don't hold locks across await points
- [`anti-string-for-str`](rules/anti-string-for-str.md) - Don't accept &String when &str works
- [`anti-vec-for-slice`](rules/anti-vec-for-slice.md) - Don't accept &Vec<T> when &[T] works
- [`anti-index-over-iter`](rules/anti-index-over-iter.md) - Don't use indexing when iterators work
- [`anti-panic-expected`](rules/anti-panic-expected.md) - Don't panic on expected or recoverable errors
- [`anti-empty-catch`](rules/anti-empty-catch.md) - Don't silently ignore errors
- [`anti-over-abstraction`](rules/anti-over-abstraction.md) - Don't over-abstract with excessive generics
- [`anti-premature-optimize`](rules/anti-premature-optimize.md) - Don't optimize before profiling
- [`anti-type-erasure`](rules/anti-type-erasure.md) - Don't use Box<dyn Trait> when impl Trait works
- [`anti-format-hot-path`](rules/anti-format-hot-path.md) - Don't use format! in hot paths
- [`anti-collect-intermediate`](rules/anti-collect-intermediate.md) - Don't collect intermediate iterators
- [`anti-stringly-typed`](rules/anti-stringly-typed.md) - Don't use strings where enums or newtypes would provide type safety

---

## How to Use

This skill provides rule identifiers for quick reference. When generating or reviewing Rust code:

1. **Check relevant category** based on task type
2. **Apply rules** with matching prefix
3. **Prioritize** CRITICAL > HIGH > MEDIUM > LOW
4. **Read rule files** in `rules/` for detailed examples

### Rule Application by Task

| Task | Primary Categories |
|------|-------------------|
| New function | `err-`, `name-`, `pat-` |
| New struct/API | `api-`, `type-` |
| Async code | `async-` |
| Error handling | `err-`, `api-`, `pat-` |
| Serialization (serde) | `serde-`, `type-`, `api-` |
| Logging / observability | `obs-`, `err-` |
| Tests | `test-` |
| Module layout | `proj-`, `name-` |
| Code review | `anti-`, `lint-` |

---

## Sources & Attribution

This skill is an independent synthesis of official Rust guidance, well-known books, and patterns from widely-used crates. It is not affiliated with or endorsed by the Rust project or any crate author; the text and code examples are original.

**Official Rust documentation**
- [The Rust Reference](https://doc.rust-lang.org/reference/)
- [Rust API Guidelines](https://rust-lang.github.io/api-guidelines/)
- [The Rustonomicon](https://doc.rust-lang.org/nomicon/) (unsafe code)
- [Rust 2024 Edition Guide](https://doc.rust-lang.org/edition-guide/rust-2024/)
- [The Cargo Book](https://doc.rust-lang.org/cargo/)
- [Standard library docs](https://doc.rust-lang.org/std/) and [release notes](https://doc.rust-lang.org/releases.html)

**Books & guides**
- [The Rust Performance Book](https://nnethercote.github.io/perf-book/) — Nicholas Nethercote
- [Rust Design Patterns](https://rust-unofficial.github.io/patterns/) — rust-unofficial
- [Rust Atomics and Locks](https://marabos.nl/atomics/) — Mara Bos
- [Effective Rust](https://effective-rust.com/) — David Drysdale

**Tooling**
- [Clippy lint documentation](https://rust-lang.github.io/rust-clippy/)
- [Miri](https://github.com/rust-lang/miri)

**Real-world codebases studied for idioms**
- ripgrep, tokio, serde, clap, polars, axum, cargo, hyper, bevy, rayon, and dtolnay's crates (thiserror, anyhow, syn)

This project is MIT-licensed. Referenced upstream materials remain under their own licenses (the official Rust docs and API Guidelines are dual MIT / Apache-2.0).
