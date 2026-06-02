fn main() {
    println!("cargo:rerun-if-env-changed=WAX_BUILD_VERSION");
}
