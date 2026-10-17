//! Writes the `OpenAPI` spec to stdout; `just openapi` saves it as `crates/triage/openapi.json`.

use std::io::Write as _;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    std::io::stdout().write_all(triage::http::api_json()?.as_bytes())?;
    Ok(())
}
