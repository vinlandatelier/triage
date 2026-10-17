use crate::helpers::{TestResult, triage};

#[test]
fn config_show_creates_nothing() -> TestResult {
    let tmp = tempfile::tempdir()?;
    let data = tmp.path().join("fresh");
    let out = triage(&data)
        .current_dir(tmp.path())
        .args(["config", "show"])
        .output()?;
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(out.status.code(), Some(0), "{stderr}");
    assert!(String::from_utf8(out.stdout)?.contains(&data.display().to_string()));
    assert!(!data.exists());
    Ok(())
}
