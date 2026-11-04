//! Does the shader actually compile? Ours.
//!
//! WHY THIS EXISTS
//!
//! WGSL is compiled when the app runs, not when it is built. So a mistake in it
//! passes `cargo build`, passes `cargo test`, passes the production build, and
//! then every photo opens black — which is exactly what happened,
//! twice in one evening. There was nothing between writing a bad line and
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
/// Not a second copy of the concatenation: it is the same constant the renderer
/// compiles. This file used to hold its own `concat!` of the same two includes,
/// which was fine until a third caller appeared and the whole point of the check
/// (that the text under test is the text that runs) stopped being guaranteed by
/// anything but everyone remembering.
#[cfg(test)]
pub use super::export_precision::SHADER_SOURCE as SOURCE;

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

    fn compact_shader(source: &str) -> String {
        source.chars().filter(|ch| !ch.is_whitespace()).collect()
    }

    #[test]
    fn neutral_brightness_is_an_exact_noop() {
        assert!(compact_shader(SOURCE).contains("if(brightness_adj==0.0){returncolor_in;}"));
    }

    #[test]
    fn neutral_highlights_are_an_exact_noop() {
        assert!(compact_shader(SOURCE).contains("if(abs(highlights_adj)<0.001){returncolor_in;}"));
    }

    #[test]
    fn neutral_vibrance_is_an_exact_noop() {
        assert!(compact_shader(SOURCE).contains("if(sat==0.0&&vib==0.0){returncolor;}"));
    }

    /// A straight-line curve reaches the shader with no points
    /// (image_processing.rs, `shader_curve`), and this is what passes it through.
    #[test]
    fn identity_rgb_curves_bypass_curve_interpolation() {
        assert!(compact_shader(SOURCE).contains(
            "fnapply_curve(val:f32,src:u32,ch:u32,count:u32)->f32{if(count<2u){returnval;}"
        ));
    }

    /// No function in the render shader takes an array, or a struct holding
    /// one, as an argument.
    ///
    /// wgpu copies such an argument to the stack when the function indexes it
    /// with a variable, once per call and so once per pixel. On an Intel Arc
    /// 140T that copy was most of the render on Vulkan and DirectX 12: a
    /// 3328x2219 slider move took 676 ms on Vulkan with the curves and the HSL
    /// bands passed that way, and 15 ms once they were read from the
    /// adjustments buffer and passed as vectors. OpenGL hid it (156 ms), which
    /// is why it went unseen while Auto picked Vulkan.
    #[test]
    fn no_function_takes_an_array_by_value() {
        use wgpu::naga::{Handle, Module, Type, TypeInner};
        fn holds_array(module: &Module, ty: Handle<Type>) -> bool {
            match &module.types[ty].inner {
                TypeInner::Array { .. } | TypeInner::BindingArray { .. } => true,
                TypeInner::Struct { members, .. } => {
                    members.iter().any(|m| holds_array(module, m.ty))
                }
                _ => false,
            }
        }
        let module = wgpu::naga::front::wgsl::parse_str(SOURCE).expect("parses");
        let offenders: Vec<String> = module
            .functions
            .iter()
            .flat_map(|(_, f)| {
                f.arguments
                    .iter()
                    .filter(|a| holds_array(&module, a.ty))
                    .map(move |a| {
                        format!(
                            "{}({})",
                            f.name.as_deref().unwrap_or("?"),
                            a.name.as_deref().unwrap_or("?")
                        )
                    })
            })
            .collect();
        assert!(
            offenders.is_empty(),
            "pass an index into `adjustments` or vectors instead: {offenders:?}"
        );
    }

    /// No shader writes into a vector or matrix held in a function's own
    /// variable through an index known only when it runs (`v[i] = x`).
    ///
    /// FXC, the HLSL compiler DirectX 12 uses on every Windows PC, cannot
    /// address such a write, so it unrolls the loop around it; inside a loop
    /// it cannot unroll, the pipeline fails to build. Lightroom's grading
    /// wrote `out[c]` inside the per-mask loop, and every photo failed on
    /// DirectX 12 while Vulkan and OpenGL were fine. Build the vector from
    /// its parts instead. A local array is fine: FXC can index those, and
    /// RapidRAW's noise reduction writes one.
    #[test]
    fn no_shader_writes_a_local_vector_through_a_variable_index() {
        use wgpu::naga::{Block, Expression, Function, Handle, Module, Statement, TypeInner};
        fn into_local_vector(m: &Module, f: &Function, mut e: Handle<Expression>) -> bool {
            let mut variable_index = false;
            loop {
                match f.expressions[e] {
                    Expression::Access { base, .. } => (variable_index, e) = (true, base),
                    Expression::AccessIndex { base, .. } => e = base,
                    Expression::LocalVariable(v) => {
                        return variable_index
                            && matches!(
                                m.types[f.local_variables[v].ty].inner,
                                TypeInner::Vector { .. } | TypeInner::Matrix { .. }
                            );
                    }
                    _ => return false,
                }
            }
        }
        fn writes(m: &Module, f: &Function, block: &Block) -> usize {
            block
                .iter()
                .map(|s| match s {
                    Statement::Store { pointer, .. } => {
                        usize::from(into_local_vector(m, f, *pointer))
                    }
                    Statement::Block(b) => writes(m, f, b),
                    Statement::If { accept, reject, .. } => {
                        writes(m, f, accept) + writes(m, f, reject)
                    }
                    Statement::Switch { cases, .. } => {
                        cases.iter().map(|c| writes(m, f, &c.body)).sum()
                    }
                    Statement::Loop {
                        body, continuing, ..
                    } => writes(m, f, body) + writes(m, f, continuing),
                    _ => 0,
                })
                .sum()
        }
        let shaders = [
            ("render", SOURCE),
            ("display", DISPLAY_SOURCE),
            ("sharpen", crate::mods::sharpen_gpu::SHADER),
            ("blur", include_str!("../shaders/blur.wgsl")),
            ("flare", include_str!("../shaders/flare.wgsl")),
            ("guided", include_str!("../shaders/guided.wgsl")),
        ];
        let mut offenders = Vec::new();
        for (what, source) in shaders {
            let module = wgpu::naga::front::wgsl::parse_str(source).expect("parses");
            let functions = module.functions.iter().map(|(_, f)| f);
            for f in functions.chain(module.entry_points.iter().map(|e| &e.function)) {
                if writes(&module, f, &f.body) > 0 {
                    offenders.push(format!("{what}: {}", f.name.as_deref().unwrap_or("?")));
                }
            }
        }
        assert!(
            offenders.is_empty(),
            "build the vector from its parts instead: {offenders:?}"
        );
    }

    /// The export pipeline compiles too.
    ///
    /// This is the check the feature most needs and the one a GPU-free test can
    /// still make. `export_shader_source` rewrites the storage declaration to
    /// `rgba32float` by text and the dither is gated behind an `override`; either
    /// could produce a shader that parses and then fails to validate, and the
    /// first anyone would hear of it is a user pressing Export. naga is the same
    /// compiler wgpu will use, so a failure here is the failure they would get.
    #[test]
    fn the_export_shader_compiles() {
        let source = crate::mods::export_precision::export_shader_source()
            .expect("the storage declaration should still be found");
        compiles(&source, "high-precision export");
        assert!(
            source.contains("rgba32float, write>"),
            "the export shader compiled, but at 8 bits",
        );
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
        assert_eq!(
            SOURCE.matches("= ag_stage_grade(").count(),
            1,
            "the grading stage should be called exactly once"
        );
        assert_eq!(
            SOURCE.matches("= ag_stage_color(").count(),
            1,
            "the colour stage (B&W) should be called exactly once"
        );
        assert_eq!(
            SOURCE.matches("linear_to_vlog(ag_film_lut_input(").count(),
            1,
            "a film look's input (mods/film_lut.rs) should be called exactly once, inside the V-Log encode"
        );
    }

    /// The adjustments buffer is written from Rust and read by WGSL, and the
    /// two sides are kept in step by hand. A field added to one and not the
    /// other - or added with a different size - shifts everything after it,
    /// and the shader reads garbage with no error anywhere. Argentum adds
    /// fields to their struct (the camera profile, RAW tone, sharpening,
    /// grading, B&W, look grain), so this compares the sizes naga computes with
    /// the sizes Rust lays out.
    #[test]
    fn the_adjustments_have_the_same_layout_on_both_sides() {
        let module = wgpu::naga::front::wgsl::parse_str(SOURCE).expect("parses");
        let mut layouter = wgpu::naga::proc::Layouter::default();
        layouter.update(module.to_ctx()).expect("lays out");
        let size_of = |name: &str| {
            let (handle, _) = module
                .types
                .iter()
                .find(|(_, t)| t.name.as_deref() == Some(name))
                .unwrap_or_else(|| panic!("no struct {name} in the shader"));
            layouter[handle].size as usize
        };
        assert_eq!(
            size_of("AgSharpen"),
            std::mem::size_of::<crate::mods::sharpen::Params>()
        );
        assert_eq!(
            size_of("AgGrade"),
            std::mem::size_of::<crate::mods::color_grade::Params>()
        );
        assert_eq!(
            size_of("AgBlackWhite"),
            std::mem::size_of::<crate::mods::black_white::Params>()
        );
        assert_eq!(
            size_of("AgLookGrain"),
            std::mem::size_of::<crate::mods::look_grain::Params>()
        );
        assert_eq!(
            size_of("GlobalAdjustments"),
            std::mem::size_of::<crate::image_processing::GlobalAdjustments>()
        );
        assert_eq!(
            size_of("AllAdjustments"),
            std::mem::size_of::<crate::image_processing::AllAdjustments>()
        );
    }
}
