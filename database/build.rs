fn main() {
    // sqlx::migrate! embeds SQL at compile time. New files must invalidate the
    // cached database crate even when no Rust source has changed.
    println!("cargo:rerun-if-changed=migrations");
}
