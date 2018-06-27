extern crate bindgen;

use std::env;
use std::path::PathBuf;

fn main() {
    println!("cargo:rustc-link-lib=avformat");
    println!("cargo:rustc-link-lib=swscale");
    println!("cargo:rustc-link-lib=avcodec");
    println!("cargo:rustc-link-lib=avutil");

    // Tell cargo to tell rustc to link the system bzip2
    // shared library.
    println!("cargo:rustc-link-lib=bz2");

    // The bindgen::Builder is the main entry point
    // to bindgen, and lets you build up options for
    // the resulting bindings.
    let bindings = bindgen::Builder::default()
        // The input header we would like to generate
        // bindings for.
        .header("../elevator/libsourcey/src/webrtc/include/webrtcelevator.h")
        // Finish the builder and generate the bindings.
        .generate()
        // Unwrap the Result and panic on failure.
        .expect("Unable to generate bindings");

    // Write the bindings to the $OUT_DIR/bindings.rs file.
    let out_path = PathBuf::from(env::var("OUT_DIR").unwrap());
    bindings
        .write_to_file(out_path.join("webrtcelevator.rs"))
        .expect("Couldn't write bindings!");

    println!("cargo:rustc-link-search=native=../elevator/libsourcey/cmake-build-debug/lib");
    println!("cargo:rustc-link-search=native=../elevator/libwebrtc/src/out/Debug/obj");
    println!("cargo:rustc-link-search=native=../elevator/libwebrtc/src/out/Debug/obj/third_party/boringssl");
    println!("cargo:rustc-link-search=native=../elevator/libwebrtc/src/out/Debug/obj/third_party/protobuf");
    for lib in [
        "webrtcelevator",

        "scy_base",
        "scy_crypto",
        "scy_net",
        "scy_http",
        "scy_util",
        "scy_json",
        "scy_socketio",
        "scy_symple",
        "scy_webrtc",

        "webrtc",

        "stdc++",
        "libuv",
        "minizip",
        "http_parser",

        "rt",
        "z",
    ].iter() {
        println!("cargo:rustc-link-lib=static={}", lib);
    }

    for lib in [
        "m",
        "dl",
        "X11",
        "Xext",
    ].iter() {
        println!("cargo:rustc-link-lib=dylib={}", lib);
    }
}
