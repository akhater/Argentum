//! Runs the registered tools, and keeps what each one made.
//!
//! Every step's result is cached under a key built from the key of its input
//! and the step's own settings. Changing one tool's settings therefore
//! changes its key and every key after it, and nothing before: a render finds
//! the last step it still has a result for and runs only what comes after.
//!
//! The steps are the registered tools in order, with the develop (sensor data
//! to linear RGB) between the last `Raw` tool and the first `SceneLinear` one,
//! and, for a preview, a resize straight after the develop. So a preview and
//! a full-size render share everything up to the develop, and a preview's
//! tools run on the small picture.
//!
//! Each run of consecutive per-pixel tools (`Tool::PIXEL`) is one step, the
//! pixel pass (`src/pixels/`): one GPU dispatch for all of them. A view's
//! frame comes straight out of the last run, cut and encoded on the GPU, so a
//! slider tick runs that dispatch and reads back what is shown, nothing more.

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::ops::Range;
use std::sync::Arc;
use std::time::{Duration, Instant};

use serde_json::Value;

use crate::cache::Cache;
use crate::contract::{EditDocument, EditPatch, RenderTicket, Stage};
use crate::develop::{self, DEVELOP_VERSION};
use crate::frame::{Frame, Layout};
use crate::pixels::{Params, Pixels};
use crate::registry::Registry;
use crate::tool::{self, AnySettings, Context, Tool};
use crate::{Error, GpuSlot, Image, Result, Rgb, Source};

/// How big a render is.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Size {
    Full,
    /// As large as fits in this many pixels across and down, keeping the
    /// photo's shape; never larger than full size.
    Fit(u32, u32),
}

/// One step of the last render.
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// The tool's id, or `develop`, `resize`, `pixels`.
    pub name: &'static str,
    /// Whether it ran. A step that did not run was served from the cache, or
    /// was not needed because a later step was.
    pub ran: bool,
    pub time: Duration,
}

/// What one step of a render does.
#[derive(Clone, Copy)]
enum Op {
    Tool(usize),
    Develop,
    Resize(u32, u32),
    /// The per-pixel tools from the first to before the second, in one pass.
    Pixels(usize, usize),
}

/// The default memory the cache may hold. A full-size render of a 32 MP
/// photo keeps about 1.4 GB: the decoded mosaic, the developed picture and
/// each tool's result after it.
pub const DEFAULT_CACHE_BYTES: usize = 2 << 30;

pub struct Pipeline {
    registry: Arc<Registry>,
    settings: Vec<Box<dyn AnySettings>>,
    hashes: Vec<u64>,
    cache: Cache,
    gpu: GpuSlot,
    last: Vec<Step>,
    runs: HashMap<&'static str, u32>,
    pixels: Pixels,
}

impl Pipeline {
    /// A pipeline running `registry`'s tools, every one at its defaults.
    pub fn new(registry: Registry) -> Result<Pipeline> {
        Self::with_gpu(Arc::new(registry), GpuSlot::new())
    }

    /// The same, sharing `registry`, with GPU tools using this device rather
    /// than opening one.
    pub fn with_gpu(registry: Arc<Registry>, gpu: GpuSlot) -> Result<Pipeline> {
        registry.check()?;
        let settings: Vec<_> = registry.tools.iter().map(|t| t.defaults()).collect();
        let hashes = registry
            .infos()
            .iter()
            .zip(&settings)
            .map(|(info, s)| settings_hash(info.id, info.version, s.as_ref()))
            .collect();
        Ok(Pipeline {
            registry,
            settings,
            hashes,
            cache: Cache::new(DEFAULT_CACHE_BYTES),
            gpu,
            last: Vec::new(),
            runs: HashMap::new(),
            pixels: Pixels::default(),
        })
    }

    /// Argentum's tools.
    pub fn standard() -> Result<Pipeline> {
        Self::new(crate::registry::standard())
    }

    pub fn registry(&self) -> &Registry {
        &self.registry
    }

    /// Set `T`'s settings, clamped to its ranges.
    pub fn set<T: Tool>(&mut self, settings: T::Settings) -> Result<()> {
        let i = self.registry.position_of::<T>().ok_or_else(|| {
            Error::Registry(format!("{} is not in this pipeline's registry", T::ID))
        })?;
        self.store(i, Box::new(tool::clamp(settings)));
        Ok(())
    }

    /// `T`'s settings.
    pub fn get<T: Tool>(&self) -> Option<&T::Settings> {
        let i = self.registry.position_of::<T>()?;
        self.settings[i].as_any().downcast_ref()
    }

    /// Set a tool's settings from saved JSON, by id: migrated if `version` is
    /// older than the tool's, refused if it is newer, clamped either way.
    pub fn set_json(&mut self, id: &str, version: u32, value: Value) -> Result<()> {
        let i = self
            .registry
            .position(id)
            .ok_or_else(|| Error::UnknownTool(id.to_string()))?;
        let settings = self.registry.tools[i].read(version, value)?;
        self.store(i, settings);
        Ok(())
    }

    /// Every tool's settings, as an edit saves them.
    pub fn edit(&self) -> EditDocument {
        self.registry.edit_of(&self.settings)
    }

    /// Every tool's settings from an edit; a tool it does not mention is at
    /// its defaults. All or nothing: when any tool's settings cannot be read,
    /// nothing changes. A tool whose settings are the same as before keeps
    /// its cached results.
    pub fn set_edit(&mut self, doc: &EditDocument) -> Result<()> {
        let next = self.registry.read_edit(doc)?;
        for (i, settings) in next.into_iter().enumerate() {
            self.store(i, settings);
        }
        Ok(())
    }

    /// Merge `patch` into the settings, as `Registry::merge` does, `null`
    /// meaning the settings' own default. Only the tools it names change, so
    /// only they and the tools after them run again.
    pub fn apply(&mut self, patch: &EditPatch) -> Result<()> {
        let doc = self
            .registry
            .merge(&self.edit(), patch, &self.registry.default_edit())?;
        self.set_edit(&doc)
    }

    fn store(&mut self, i: usize, settings: Box<dyn AnySettings>) {
        let info = &self.registry.infos()[i];
        self.hashes[i] = settings_hash(info.id, info.version, settings.as_ref());
        self.settings[i] = settings;
    }

    /// The memory the cache may hold. Least recently used results go first.
    pub fn set_cache_budget_bytes(&mut self, bytes: usize) {
        self.cache.set_budget_bytes(bytes);
    }

    /// The memory the cache holds now.
    pub fn cache_bytes(&self) -> usize {
        self.cache.used_bytes()
    }

    /// The steps of the last render, in order.
    pub fn last_render(&self) -> &[Step] {
        &self.last
    }

    /// How many times a step has run since this pipeline was made.
    pub fn runs(&self, name: &str) -> u32 {
        self.runs.get(name).copied().unwrap_or(0)
    }

    /// The photo with every tool applied, at `size`.
    pub fn render(&mut self, source: &Source, size: Size) -> Result<Arc<Image>> {
        self.render_unless(source, size, &|| false)
    }

    /// The same, giving up with `Error::Cancelled` between two steps once
    /// `cancelled` says so. What was finished stays cached.
    pub fn render_unless(
        &mut self,
        source: &Source,
        size: Size,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Arc<Image>> {
        let ops = self.ops(size);
        Ok(self.run_ops(source, &ops, cancelled)?.0)
    }

    /// `layout`'s frame of a view: every step but the last run of per-pixel
    /// tools as `render_unless` runs them, then that run straight to the
    /// frame, on the GPU. A pipeline whose last tool works on whole pictures
    /// still has its frame cut and encoded there, by a run of no tools.
    pub(crate) fn render_frame(
        &mut self,
        source: &Source,
        layout: &Layout,
        ticket: RenderTicket,
        cancelled: &dyn Fn() -> bool,
    ) -> Result<Frame> {
        let mut ops = self.ops(layout.size);
        let tools = match ops.last() {
            Some(&(Op::Pixels(first, end), _)) => {
                ops.pop();
                first..end
            }
            _ => {
                let n = self.registry.infos().len();
                n..n
            }
        };
        let (image, input_key) = self.run_ops(source, &ops, cancelled)?;
        let rgb = as_rgb("pixels", &image)?;
        let params = self.pixel_params(tools.clone(), source, input_key)?;
        let gpu = self.gpu.get()?;
        let started = Instant::now();
        let frame = self.pixels.frame(
            &gpu,
            &self.registry,
            tools,
            &params,
            rgb,
            input_key,
            layout,
            ticket,
        )?;
        *self.runs.entry("pixels").or_default() += 1;
        self.last.push(Step {
            name: "pixels",
            ran: true,
            time: started.elapsed(),
        });
        Ok(frame)
    }

    /// Every per-pixel tool at its settings, on `rgb`, as the pixel pass
    /// runs them: for trying them on a picture of one's own. `source` is the
    /// photo they take it to be.
    pub fn run_pixel_tools(&mut self, source: &Source, rgb: &Rgb) -> Result<Rgb> {
        let key = mix(&(
            "pixels",
            rgb.width,
            bytemuck::cast_slice::<[f32; 3], u8>(&rgb.pixels),
        ));
        let runs: Vec<Range<usize>> = self
            .ops(Size::Full)
            .iter()
            .filter_map(|&(op, _)| match op {
                Op::Pixels(first, end) => Some(first..end),
                _ => None,
            })
            .collect();
        let gpu = self.gpu.get()?;
        let mut out = rgb.clone();
        for tools in runs {
            let input_key = mix(&(key, tools.start));
            let params = self.pixel_params(tools.clone(), source, input_key)?;
            if let Some(next) =
                self.pixels
                    .linear(&gpu, &self.registry, tools, &params, &out, input_key)?
            {
                out = next;
            }
        }
        Ok(out)
    }

    /// The steps `ops`, from the last one with a result kept: the picture,
    /// and its key.
    fn run_ops(
        &mut self,
        source: &Source,
        ops: &[(Op, &'static str)],
        cancelled: &dyn Fn() -> bool,
    ) -> Result<(Arc<Image>, u64)> {
        let start = mix(&("source", source.key));
        let mut keys = Vec::with_capacity(ops.len());
        let mut key = start;
        for &(op, _) in ops {
            key = mix(&(key, self.op_key(op)));
            keys.push(key);
        }

        // The last step with a result already kept: everything up to it is
        // done, and only what follows runs.
        let found = (0..ops.len())
            .rev()
            .find_map(|i| self.cache.get(keys[i]).map(|image| (i, image)));
        let done = found.as_ref().map(|(i, _)| *i);
        let (mut image, mut input_key) = match found {
            Some((i, image)) => (image, keys[i]),
            None => match self.cache.get(start) {
                Some(image) => (image, start),
                None => {
                    let image = Arc::new(Image::Raw(source.raw.clone()));
                    self.cache.put(start, image.clone(), true);
                    (image, start)
                }
            },
        };

        self.last.clear();
        let first = done.map_or(0, |i| i + 1);
        for &(_, name) in &ops[..first] {
            self.last.push(Step {
                name,
                ran: false,
                time: Duration::ZERO,
            });
        }
        for (i, &(op, name)) in ops.iter().enumerate().skip(first) {
            if cancelled() {
                return Err(Error::Cancelled);
            }
            let started = Instant::now();
            let made = self.run(op, &image, source, input_key)?;
            let time = started.elapsed();
            let fresh = made.is_some();
            if let Some(made) = made {
                image = Arc::new(made);
            }
            self.cache.put(keys[i], image.clone(), fresh);
            input_key = keys[i];
            *self.runs.entry(name).or_default() += 1;
            self.last.push(Step {
                name,
                ran: true,
                time,
            });
        }
        Ok((image, input_key))
    }

    /// The steps for a render at `size`.
    fn ops(&self, size: Size) -> Vec<(Op, &'static str)> {
        let tools = self.registry.infos();
        let raw = tools.iter().take_while(|t| t.stage == Stage::Raw).count();
        let mut ops: Vec<_> = (0..raw).map(|i| (Op::Tool(i), tools[i].id)).collect();
        ops.push((Op::Develop, "develop"));
        if let Size::Fit(w, h) = size {
            ops.push((Op::Resize(w, h), "resize"));
        }
        // Each run of per-pixel tools is one step.
        let mut i = raw;
        while i < tools.len() {
            if tools[i].pixel.is_some() {
                let first = i;
                while i < tools.len() && tools[i].pixel.is_some() {
                    i += 1;
                }
                ops.push((Op::Pixels(first, i), "pixels"));
            } else {
                ops.push((Op::Tool(i), tools[i].id));
                i += 1;
            }
        }
        ops
    }

    /// The per-pixel tools `tools`' parameters for a pass on the input keyed
    /// `input_key`.
    fn pixel_params(&self, tools: Range<usize>, source: &Source, input_key: u64) -> Result<Params> {
        tools
            .map(|i| {
                let info = &self.registry.infos()[i];
                let cx = Context::for_tool(info.id, info.gpu, source, input_key, &self.gpu);
                self.registry.tools[i].pixel_params(self.settings[i].as_ref(), &cx)
            })
            .collect()
    }

    /// What, besides its input, decides a step's result.
    fn op_key(&self, op: Op) -> u64 {
        match op {
            Op::Tool(i) => self.hashes[i],
            Op::Develop => mix(&("develop", DEVELOP_VERSION)),
            Op::Resize(w, h) => mix(&("resize", w, h)),
            Op::Pixels(first, end) => mix(&("pixels", &self.hashes[first..end])),
        }
    }

    fn run(
        &mut self,
        op: Op,
        input: &Image,
        source: &Source,
        input_key: u64,
    ) -> Result<Option<Image>> {
        match op {
            Op::Develop => {
                let raw = input.as_raw().ok_or_else(|| Error::Tool {
                    tool: "develop",
                    message: format!("was handed {}", input.describe()),
                })?;
                develop::develop(raw).map(|rgb| Some(Image::Rgb(rgb)))
            }
            Op::Resize(w, h) => {
                let rgb = as_rgb("resize", input)?;
                Ok(develop::resize(rgb, w, h).map(Image::Rgb))
            }
            Op::Pixels(first, end) => {
                let rgb = as_rgb("pixels", input)?;
                let params = self.pixel_params(first..end, source, input_key)?;
                // Nothing to do needs no GPU.
                if params.iter().all(Option::is_none) {
                    return Ok(None);
                }
                let gpu = self.gpu.get()?;
                let made = self.pixels.linear(
                    &gpu,
                    &self.registry,
                    first..end,
                    &params,
                    rgb,
                    input_key,
                )?;
                Ok(made.map(Image::Rgb))
            }
            Op::Tool(i) => {
                let info = &self.registry.infos()[i];
                let fits = match info.stage {
                    Stage::Raw => input.as_raw().is_some(),
                    Stage::SceneLinear => input.as_rgb().is_some(),
                };
                if !fits {
                    return Err(Error::Tool {
                        tool: info.id,
                        message: format!(
                            "runs at {:?} and was handed {}",
                            info.stage,
                            input.describe()
                        ),
                    });
                }
                let cx = Context::for_tool(info.id, info.gpu, source, input_key, &self.gpu);
                self.registry.tools[i].process(self.settings[i].as_ref(), input, &cx)
            }
        }
    }
}

/// `image` as RGB, or what a step was handed instead.
fn as_rgb<'a>(step: &'static str, image: &'a Image) -> Result<&'a Rgb> {
    image.as_rgb().ok_or_else(|| Error::Tool {
        tool: step,
        message: format!("was handed {}", image.describe()),
    })
}

fn mix(value: &impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

/// A tool's settings, as a cache key: its id, its settings version and the
/// settings as JSON.
fn settings_hash(id: &str, version: u32, settings: &dyn AnySettings) -> u64 {
    mix(&(id, version, settings.to_json().to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn patch(json: serde_json::Value) -> EditPatch {
        serde_json::from_value(json).unwrap()
    }

    #[test]
    fn a_patch_merges_and_null_resets() {
        let mut p = Pipeline::standard().unwrap();
        p.apply(&patch(serde_json::json!({
            "rr_basic_tone": { "contrast": 15.0, "shadows": 20.0 },
        })))
        .unwrap();
        let doc = p.edit();
        let tone = &doc.tools["rr_basic_tone"].settings;
        assert_eq!(
            (tone["contrast"].as_f64(), tone["shadows"].as_f64()),
            (Some(15.0), Some(20.0))
        );
        p.apply(&patch(
            serde_json::json!({ "rr_basic_tone": { "contrast": null } }),
        ))
        .unwrap();
        let tone = &p.edit().tools["rr_basic_tone"].settings;
        assert_eq!(tone["contrast"].as_f64(), Some(0.0), "back to its default");
        assert_eq!(tone["shadows"].as_f64(), Some(20.0), "the rest kept");
    }

    #[test]
    fn a_bad_patch_changes_nothing() {
        let mut p = Pipeline::standard().unwrap();
        let before = p.edit();
        for bad in [
            serde_json::json!({ "ag_nothing": { "x": 1 } }),
            serde_json::json!({ "ag_exposure": { "contrast": 1 } }),
            serde_json::json!({ "ag_exposure": { "exposure_ev": "bright" } }),
            serde_json::json!({ "rt_capture_sharpen": { "iterations": 2.5 } }),
            // The first tool is fine, the second is not: neither changes.
            serde_json::json!({ "ag_exposure": { "exposure_ev": 1 }, "dt_highlights": { "recover": 3 } }),
        ] {
            assert!(p.apply(&patch(bad.clone())).is_err(), "{bad}");
            assert_eq!(p.edit(), before, "{bad}");
        }
    }

    #[test]
    fn an_edit_round_trips() {
        let mut p = Pipeline::standard().unwrap();
        p.apply(&patch(serde_json::json!({
            "rt_capture_sharpen": { "radius": 1.1, "autoRadius": false, "iterations": 30 },
        })))
        .unwrap();
        let doc = p.edit();
        let mut q = Pipeline::standard().unwrap();
        q.set_edit(&doc).unwrap();
        assert_eq!(q.edit(), doc);
        let mut empty = EditDocument::new();
        q.set_edit(&empty).unwrap();
        assert_eq!(
            q.edit(),
            Pipeline::standard().unwrap().edit(),
            "all defaults"
        );
        empty.version = 2;
        assert!(q.set_edit(&empty).is_err());
    }
}
