fn main() {
    // `sqlx::migrate!` embeds the files, so a changed migration must rebuild the crate.
    println!("cargo:rerun-if-changed=migrations");
}
