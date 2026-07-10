// Bake the build target triple into the binary so `fpl update` can pick the
// matching release asset (e.g. `fpl-aarch64-apple-darwin.tar.gz`).
fn main() {
    let target = std::env::var("TARGET").unwrap_or_default();
    println!("cargo:rustc-env=FPL_TARGET={target}");
}
