fn main() {
    // `sqlx::migrate!` embeds the files, so a changed migration must rebuild the crate.
    println!("cargo:rerun-if-changed=migrations");
    // rust-embed resolves the folder at compile time, so a build made before `web/dist/`
    // existed would never serve the UI.
    println!("cargo:rerun-if-changed=../../web/dist");
}
