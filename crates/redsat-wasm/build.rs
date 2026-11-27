//! WebAssembly's stack lives in linear memory and defaults to 1 MiB, too little for the
//! tree-walking interpreter's deep recursion (natively, programs run on a 1 GiB thread stack).

fn main() {
    if std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        println!("cargo:rustc-link-arg=-zstack-size=67108864");
    }
}
