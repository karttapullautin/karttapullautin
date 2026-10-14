// wasm32-unknown-unknown has no OS, so std has no clock there. Run with:
//   cargo install wasm-bindgen-cli --version <wasm-bindgen version in Cargo.lock>
//   CARGO_TARGET_WASM32_UNKNOWN_UNKNOWN_RUNNER=wasm-bindgen-test-runner \
//     cargo test --target wasm32-unknown-unknown --test wasm_unknown
#![cfg(all(target_family = "wasm", target_os = "unknown"))]

use pullauta::util::Timing;
use wasm_bindgen_test::wasm_bindgen_test;

// Every run starts a Timing (process_tile, process_zip), so this is the first
// clock read a browser build makes.
#[wasm_bindgen_test]
fn timing_works_without_an_os_clock() {
    let mut timing = Timing::start_now("wasm");
    timing.start_section("section");
    assert!(timing.end_section().is_some());
}
