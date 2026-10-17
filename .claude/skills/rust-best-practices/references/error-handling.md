# Error Handling in Rust

## When to Use This Reference

- Deciding between `Result` and `panic!`
- Adding an error variant or mapping a failure into `AppError`
- Reviewing code with `.unwrap()` or `.expect()` calls
- Designing error types for a library

## Quick Reference

| Context | Approach |
|---------|----------|
| Feature and handler code | `crate::error::Result<T>` with `AppError` |
| Startup errors (config, http, store) | the module's own `thiserror` enum, wrapped by `cli::Error` |
| `main.rs`, `cli.rs`, `build.rs` | `anyhow` allowed |
| Unexpected failures inside a feature | `AppError::Internal(err.into())` |
| Tests | return `Result` and use `?`; `.unwrap()` and `.expect()` are denied |

`AppError` is the only error type that crosses a feature boundary; each variant maps to one RFC 9457 problem response. `.unwrap()` and `.expect()` are clippy-denied in every target, so always use `?`.

## Patterns

### Module Error Type

```rust
use thiserror::Error;

#[derive(Debug, Error)]
pub enum MyError {
    #[error("invalid input: {0}")]
    InvalidInput(String),
    #[error("connection failed")]
    Connection(#[from] std::io::Error),
}
```

### Application Error Handling

```rust
use crate::error::{AppError, Result};

pub async fn get(store: &Store, id: HelloId) -> Result<Hello> {
    repo::get(&store.read, id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("no hello {id}")))
}

fn timestamp(at: OffsetDateTime) -> Result<String> {
    at.format(TIMESTAMP).map_err(|err| AppError::Internal(err.into()))
}
```

### Never in Production Runtime

```rust
// ❌ BAD - runtime code
fn process_request(data: &str) -> Response {
    let parsed = parse(data).unwrap(); // NO
}

// ✅ GOOD - propagate error
fn process_request(data: &str) -> Result<Response> {
    let parsed = parse(data)?;
    // ...
}
```

## STOP and Reconsider

**Before reaching for `.unwrap()` or `.expect()`:** clippy denies both. Use `?`, map the failure into an `AppError` variant, or restructure so the failure cannot happen.

**Before using `panic!()` in library code:** Libraries should return errors, not crash the caller's program. Use `panic!()` only for programming errors that indicate a bug (violated invariants), never for recoverable conditions.

**Before using `.ok()` to discard an error:** Are you sure you don't need to log this? Silently swallowed errors are the hardest bugs to diagnose.

### Infallible Parsing Pattern

When a conversion is infallible (the error type is `Infallible` or `!`), use irrefutable `let` binding instead of `.unwrap()`:

```rust
use std::convert::Infallible;

fn parse_infallible(input: &str) -> Result<String, Infallible> {
    Ok(input.to_uppercase())
}

// ❌ BAD - unwrap on a Result that can never fail
let result = parse_infallible("hello").unwrap();

// ✅ GOOD - irrefutable let binding for Result<T, Infallible>
let Ok(result) = parse_infallible("hello");
// `result` is available directly — no unwrap needed
```

This works because `Infallible` is uninhabited (has no values), so the `Err` case can never occur and the compiler knows the `let Ok(x) = expr;` pattern is irrefutable.

## Common Mistakes

| Mistake | Fix |
|---------|-----|
| `.unwrap()` or `.expect()` anywhere, tests included | Use `?` or proper error handling |
| `anyhow` in library public API | Use `thiserror` for typed errors |
| Swallowing errors with `.ok()` | Log or propagate |
| Generic error messages | Use a specific `AppError` variant with a message that names the input |

## Hidden Panic Sources

Some operations panic unexpectedly. Watch for these:

| Operation | Panic Condition | Safe Alternative |
|-----------|-----------------|------------------|
| `&s[..n]` on strings | `n` splits UTF-8 char | `s.get(..n)` or `char_indices()` |
| `vec[i]` | Out of bounds | `vec.get(i)` |
| `slice.split_at(n)` | `n > len` | Check length first |
| Integer arithmetic | Overflow in debug | `checked_add()`, `saturating_add()` |

## Resources

- thiserror: https://docs.rs/thiserror
- anyhow: https://docs.rs/anyhow
- Rust Book - Error Handling: https://doc.rust-lang.org/book/ch09-00-error-handling.html
- Rust by Example - Errors: https://doc.rust-lang.org/rust-by-example/error.html
