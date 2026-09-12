//! Does the shader actually compile? Ours.
//!
//! WHY THIS EXISTS
//!
//! WGSL is compiled when the app runs, not when it is built. So a mistake in it
//! passes `cargo build`, passes `cargo test`, passes the production build, and
//! then every photo opens black — which is exactly what happened, in front of
//! AK, twice in one evening. There was nothing between writing a bad line and
//! seeing a black picture with "Validation Error" in the log and no detail.
//!
//! naga is the compiler wgpu already uses, and it is reachable through wgpu, so
//! this runs it over the same two files joined the same way `gpu_processing.rs`
//! joins them. A syntax error or a type error now fails a test with the line
//! number instead of blanking the app.
//!
//! It does not need a GPU and takes milliseconds.

/// The shader source, exactly as `gpu_processing.rs` assembles it.
///
/// Concatenated in the same order, because an error's line number is only
/// useful if the file it points into is the file that ran.
#[cfg(test)]
pub const SOURCE: &str = concat!(
    include_str!("../shaders/modules.wgsl"),
    include_str!("../shaders/shader.wgsl"),
);

/// The display shader, assembled the same way.
///
/// A second module, compiled separately at runtime, and just as capable of
/// blanking the window if it does not parse.
#[cfg(test)]
pub const DISPLAY_SOURCE: &str = concat!(
    include_str!("../shaders/ag_display.wgsl"),
    include_str!("../shaders/display.wgsl"),
);

#[cfg(test)]
mod tests {
    use super::*;

    fn compiles(source: &str, what: &str) {
        let module = match wgpu::naga::front::wgsl::parse_str(source) {
            Ok(module) => module,
            Err(e) => panic!(
                "the {what} shader does not parse:
{}",
                e.emit_to_string(source)
            ),
        };
        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );
        if let Err(e) = validator.validate(&module) {
            panic!(
                "the {what} shader does not validate:
{}",
                e.emit_to_string(source)
            );
        }
    }

    #[test]
    fn the_display_shader_compiles() {
        compiles(DISPLAY_SOURCE, "display");
    }

    #[test]
    fn the_display_stage_is_wired() {
        assert!(
            DISPLAY_SOURCE.contains("fn ag_stage_present"),
            "the presentation stage is gone"
        );
        assert_eq!(
            DISPLAY_SOURCE.matches("ag_stage_present(").count(),
            2,
            "its definition and the single call that presents every pixel"
        );
    }

    #[test]
    fn the_shader_compiles() {
        let module = match wgpu::naga::front::wgsl::parse_str(SOURCE) {
            Ok(module) => module,
            Err(e) => panic!("the shader does not parse:\n{}", e.emit_to_string(SOURCE)),
        };

        let mut validator = wgpu::naga::valid::Validator::new(
            wgpu::naga::valid::ValidationFlags::all(),
            wgpu::naga::valid::Capabilities::all(),
        );

        if let Err(e) = validator.validate(&module) {
            panic!(
                "the shader does not validate:\n{}",
                e.emit_to_string(SOURCE)
            );
        }
    }

    /// The stages exist and are called, which is the whole mergeability
    /// arrangement: their file calls two functions of ours and nothing else.
    #[test]
    fn both_stages_are_wired() {
        assert!(
            SOURCE.contains("fn ag_stage_scene_linear"),
            "the scene-linear stage is gone"
        );
        assert!(
            SOURCE.contains("fn ag_stage_display"),
            "the display stage is gone"
        );
        assert_eq!(
            SOURCE.matches("= ag_stage_scene_linear(").count(),
            1,
            "the scene-linear stage should be called exactly once"
        );
        assert_eq!(
            SOURCE.matches("= ag_stage_display(").count(),
            1,
            "the display stage should be called exactly once"
        );
    }
}
