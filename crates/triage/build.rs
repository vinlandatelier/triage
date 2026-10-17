fn main() {
    // `sqlx::migrate!` embeds the files, so a changed migration must rebuild the crate.
    println!("cargo:rerun-if-changed=migrations");
    // rust-embed resolves the folder at compile time, so a debug build made before `web/dist/`
    // existed would never serve the UI. Creating it keeps the path valid, and a missing
    // rerun-if-changed path would rerun this script on every build. Release must still fail
    // without a web build, so only debug creates it.
    if std::env::var("PROFILE").as_deref() == Ok("debug") {
        let _ = std::fs::create_dir_all("../../web/dist");
    }
    println!("cargo:rerun-if-changed=../../web/dist");
}
