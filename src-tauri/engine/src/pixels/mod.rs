//! The pixel pass: each run of consecutive per-pixel tools (`Tool::PIXEL`),
//! on the GPU, in one dispatch.
//!
//! The structure is the app's (`gpu_processing.rs` @ c786c146: `run`, and
//! the input texture and guided filter kept in `GpuImageCache`; the `main`
//! of `shader.wgsl`): the picture goes up once, as half floats, and stays,
//! with its guided filter worked out beside it; each render after that
//! writes the tools' parameters, runs one shader in which the tools follow
//! one another on each pixel, and reads back only what is shown. Ours: the
//! shader's `main` is written from the registry - each tool brings its own
//! function and is called in registry order - where the app's lists its
//! adjustments by hand; and a view's frame is cut, zoomed and encoded in the
//! same dispatch.

mod gpu;

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::ops::Range;
use std::sync::Arc;

use crate::contract::RenderTicket;
use crate::frame::{Frame, Layout};
use crate::registry::Registry;
use crate::tool::Pixel;
use crate::{Error, Gpu, Result, Rgb};

const PASS: &str = include_str!("pass.wgsl");
const COMMON: &str = include_str!("common.wgsl");

/// A pass's parameters, tool by tool: `None` for a tool its settings leave
/// alone, which the pass hands zeros.
pub(crate) type Params = Vec<Option<Vec<[f32; 4]>>>;

/// The pixel passes of one pipeline: one per run of the registry, each
/// built the first time it runs, keeping the picture it last ran on.
#[derive(Default)]
pub(crate) struct Pixels {
    runs: HashMap<(usize, usize), gpu::Run>,
    /// The guided filter's passes, shared by every run that needs them.
    guided: Option<gpu::Guided>,
}

impl Pixels {
    /// The tools `tools` of `registry` on `input`, linear light out, or
    /// `None` when every one of them leaves it alone.
    pub fn linear(
        &mut self,
        gpu: &Arc<Gpu>,
        registry: &Registry,
        tools: Range<usize>,
        params: &[Option<Vec<[f32; 4]>>],
        input: &Rgb,
        input_key: u64,
    ) -> Result<Option<Rgb>> {
        if params.iter().all(Option::is_none) {
            return Ok(None);
        }
        let (run, guided) = self.run(gpu, registry, tools)?;
        let packed = pack(&run.shader, params);
        run.linear(guided, &packed, input, input_key).map(Some)
    }

    /// The same, straight to `layout`'s frame of a view: cut, zoomed and
    /// encoded for the screen.
    #[allow(clippy::too_many_arguments)]
    pub fn frame(
        &mut self,
        gpu: &Arc<Gpu>,
        registry: &Registry,
        tools: Range<usize>,
        params: &[Option<Vec<[f32; 4]>>],
        input: &Rgb,
        input_key: u64,
        layout: &Layout,
        ticket: RenderTicket,
    ) -> Result<Frame> {
        let (run, guided) = self.run(gpu, registry, tools)?;
        let packed = pack(&run.shader, params);
        run.frame(guided, &packed, input, input_key, layout, ticket)
    }

    /// The run of `tools`, built the first time it is asked for, and the
    /// guided filter's passes if it reads them.
    fn run(
        &mut self,
        gpu: &Arc<Gpu>,
        registry: &Registry,
        tools: Range<usize>,
    ) -> Result<(&mut gpu::Run, Option<&gpu::Guided>)> {
        let run = match self.runs.entry((tools.start, tools.end)) {
            Entry::Occupied(built) => built.into_mut(),
            Entry::Vacant(slot) => {
                let pixels = registry.infos()[tools]
                    .iter()
                    .map(|t| {
                        t.pixel.map(|p| (t.id, p)).ok_or_else(|| {
                            Error::Registry(format!("{} is not a per-pixel tool", t.id))
                        })
                    })
                    .collect::<Result<Vec<_>>>()?;
                slot.insert(gpu::Run::new(gpu, source(&pixels))?)
            }
        };
        let guided = if run.shader.detail {
            Some(&*self.guided.get_or_insert_with(|| gpu::Guided::new(gpu)))
        } else {
            None
        };
        Ok((run, guided))
    }
}

/// The pass's WGSL for some tools, and what it reads.
pub(crate) struct Shader {
    pub wgsl: String,
    /// Where each tool's parameters start.
    pub offsets: Vec<usize>,
    /// How many parameters it reads in all.
    pub params: usize,
    /// Whether a tool reads the guided filter.
    pub detail: bool,
}

/// The template with the shared helpers, each tool's WGSL, and one call per
/// tool in the order given, each handed where its parameters start.
pub(crate) fn source(tools: &[(&'static str, Pixel)]) -> Shader {
    let mut offsets = Vec::with_capacity(tools.len());
    let mut bodies = String::new();
    let mut calls = String::new();
    let mut at = 0;
    for (id, pixel) in tools {
        offsets.push(at);
        bodies.push_str(&format!("// {id}\n{}\n", pixel.wgsl));
        calls.push_str(&format!("    c = {id}(c, Px(coord, input, {at}u));\n"));
        at += pixel.params;
    }
    let wgsl = PASS
        .replace("//@common", COMMON)
        .replace("//@tools", &bodies)
        .replace("//@calls", &calls);
    Shader {
        wgsl,
        offsets,
        params: at,
        detail: tools.iter().any(|(_, p)| p.detail),
    }
}

/// Every tool's parameters at its offset, zeros for a tool with none. Never
/// empty: a storage buffer cannot be.
fn pack(shader: &Shader, params: &[Option<Vec<[f32; 4]>>]) -> Vec<[f32; 4]> {
    let mut out = vec![[0.0; 4]; shader.params.max(1)];
    for (at, p) in shader.offsets.iter().zip(params) {
        if let Some(p) = p {
            out[*at..*at + p.len()].copy_from_slice(p);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    const A: Pixel = Pixel {
        wgsl: "fn ag_a(c: vec3<f32>, px: Px) -> vec3<f32> { return c; }",
        params: 2,
        detail: false,
    };
    const B: Pixel = Pixel {
        wgsl: "fn ag_b(c: vec3<f32>, px: Px) -> vec3<f32> { return c; }",
        params: 3,
        detail: true,
    };

    #[test]
    fn the_registry_writes_the_calls() {
        let shader = source(&[("ag_a", A), ("ag_b", B)]);
        assert_eq!(shader.offsets, [0, 2]);
        assert_eq!((shader.params, shader.detail), (5, true));
        let a = shader
            .wgsl
            .find("c = ag_a(c, Px(coord, input, 0u));")
            .unwrap();
        let b = shader
            .wgsl
            .find("c = ag_b(c, Px(coord, input, 2u));")
            .unwrap();
        assert!(a < b, "in registry order");
        assert!(shader.wgsl.contains(A.wgsl) && shader.wgsl.contains(B.wgsl));
        assert!(shader.wgsl.contains("fn get_luma"), "the helpers");
        assert!(!shader.wgsl.contains("//@"), "every marker filled");
    }

    #[test]
    fn the_template_marks_each_place_once() {
        for marker in ["//@common", "//@tools", "//@calls"] {
            assert_eq!(PASS.matches(marker).count(), 1, "{marker}");
        }
    }

    /// The standard tools' pass, checked as the GPU's compiler will, with no
    /// GPU: parsed and validated by naga.
    #[test]
    fn the_standard_tools_make_a_valid_shader() {
        let registry = crate::standard();
        let tools: Vec<_> = registry
            .infos()
            .iter()
            .filter_map(|t| t.pixel.map(|p| (t.id, p)))
            .collect();
        assert!(tools.len() >= 5, "{tools:?}");
        let shader = source(&tools);
        let module = naga::front::wgsl::parse_str(&shader.wgsl)
            .unwrap_or_else(|e| panic!("{}", e.emit_to_string(&shader.wgsl)));
        naga::valid::Validator::new(
            naga::valid::ValidationFlags::all(),
            naga::valid::Capabilities::all(),
        )
        .validate(&module)
        .unwrap();
    }

    #[test]
    fn a_tool_with_nothing_to_do_gets_zeros() {
        let shader = source(&[("ag_a", A), ("ag_b", B)]);
        let packed = pack(&shader, &[Some(vec![[1.0; 4]; 2]), None]);
        assert_eq!(packed[..2], [[1.0; 4]; 2]);
        assert_eq!(packed[2..], [[0.0; 4]; 3], "the whole of what it reads");
        let packed = pack(&shader, &[None, Some(vec![[2.0; 4]; 3])]);
        assert_eq!(packed[..2], [[0.0; 4]; 2]);
        assert_eq!(packed[2..], [[2.0; 4]; 3]);
        assert_eq!(pack(&source(&[]), &[]), vec![[0.0; 4]]);
    }
}
