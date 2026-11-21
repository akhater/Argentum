// Argentum: not from RAWmakase. Its own test binary, so its own process: the
// ONNX Runtime environment is process-wide and made once.
//! Loads the ONNX Runtime the app ships (`src-tauri/resources`, put there by the
//! app's build) through `load_runtime`, and checks the environment in place is
//! the one `private_environment` made, with telemetry off. Skipped, saying so,
//! where the library is not there.

use std::path::Path;

use rm_selection::runtime::{load_runtime, runtime_file_names};

#[test]
fn the_runtime_loads_into_the_environment_with_telemetry_off() {
    let library = Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../resources")
        .join(runtime_file_names()[0]);
    if !library.is_file() {
        eprintln!("skipped: no ONNX Runtime at {}", library.display());
        return;
    }
    // `private_environment` switches telemetry off on the runtime itself
    // (`DisableTelemetryEvents`): an error there would fail the load.
    load_runtime(Some(&library)).unwrap();
    // Made once per process: any later environment is refused, so the one in
    // place, which every session runs in, is the private one.
    assert!(!ort::init().commit().unwrap());
}
