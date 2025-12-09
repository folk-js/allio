//! Build script for axio crate.

fn main() {
  // Tell the linker to search for frameworks in the PrivateFrameworks directory
  // This is required for SkyLight framework (private macOS API)
  #[cfg(target_os = "macos")]
  {
    println!("cargo:rustc-link-search=framework=/System/Library/PrivateFrameworks");
  }
}
