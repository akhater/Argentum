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

use std::collections::HashMap;
use std::hash::{DefaultHasher, Hash, Hasher};
use std::path::Path;
use std::sync::Arc;
use std::time::{Duration, Instant};

use rawler::decoders::RawDecodeParams;
use rawler::rawimage::{RawImage, RawImageData};
use rawler::rawsource::RawSource;
use serde_json::Value;

use crate::contract::{EditDocument, EditPatch, Stage};
use crate::develop::{self, DEVELOP_VERSION};
use crate::registry::Registry;
use crate::tool::{self, AnySettings, Context, Tool};
use crate::{Error, GpuSlot, Image, Result};

/// A decoded photo, and the key that names it.
pub struct Source {
    /// Same key, same photo. Every cache key starts from it.
    pub key: u64,
    pub raw: RawImage,
}

impl Source {
    pub fn open(path: impl AsRef<Path>) -> Result<Source> {
        let path = path.as_ref();
        let bytes =
            std::fs::read(path).map_err(|e| Error::Decode(format!("{}: {e}", path.display())))?;
        Self::decode(&bytes)
    }

    /// Decode with rawler, keyed by the file's bytes.
    pub fn decode(bytes: &[u8]) -> Result<Source> {
        let source = RawSource::new_from_slice(bytes);
        let decoder = rawler::get_decoder(&source).map_err(|e| Error::Decode(e.to_string()))?;
        let raw = decoder
            .raw_image(&source, &RawDecodeParams::default(), false)
            .map_err(|e| Error::Decode(e.to_string()))?;
        Ok(Self::new(key_of(bytes), raw))
    }

    /// A photo decoded elsewhere. `key` has to change whenever `raw` does.
    pub fn new(key: u64, raw: RawImage) -> Source {
        Source { key, raw }
    }

    /// The developed picture's width and height, without developing it.
    pub fn size(&self) -> (usize, usize) {
        develop::developed_size(&self.raw)
    }

    /// The decoded sensor data's size in memory, in bytes.
    pub fn bytes(&self) -> usize {
        match &self.raw.data {
            RawImageData::Integer(v) => std::mem::size_of_val(v.as_slice()),
            RawImageData::Float(v) => std::mem::size_of_val(v.as_slice()),
        }
    }
}

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
    /// The tool's id, or `develop` / `resize`.
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

    /// Merge `patch` into the settings, as `Registry::merge` does. Only the
    /// tools it names change, so only they and the tools after them run
    /// again.
    pub fn apply(&mut self, patch: &EditPatch) -> Result<()> {
        let doc = self.registry.merge(&self.edit(), patch)?;
        self.set_edit(&doc)
    }

    fn store(&mut self, i: usize, settings: Box<dyn AnySettings>) {
        let info = &self.registry.infos()[i];
        self.hashes[i] = settings_hash(info.id, info.version, settings.as_ref());
        self.settings[i] = settings;
    }

    /// The memory the cache may hold, in bytes. Least recently used results
    /// go first.
    pub fn set_cache_budget(&mut self, bytes: usize) {
        self.cache.budget = bytes;
        self.cache.evict();
    }

    /// The memory the cache holds now, in bytes.
    pub fn cache_bytes(&self) -> usize {
        self.cache.used
    }

    pub fn clear_cache(&mut self) {
        self.cache = Cache::new(self.cache.budget);
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
        let start = mix(&("source", source.key));
        let mut keys = Vec::with_capacity(ops.len());
        let mut key = start;
        for &(op, _) in &ops {
            key = mix(&(key, self.op_key(op)));
            keys.push(key);
        }

        // The last step with a result already kept: everything up to it is
        // done, and only what follows runs.
        let done = (0..ops.len()).rev().find(|&i| self.cache.contains(keys[i]));
        let (mut image, mut input_key) = match done {
            Some(i) => (self.cache.get(keys[i]).expect("just found"), keys[i]),
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
        Ok(image)
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
        ops.extend((raw..tools.len()).map(|i| (Op::Tool(i), tools[i].id)));
        ops
    }

    /// What, besides its input, decides a step's result.
    fn op_key(&self, op: Op) -> u64 {
        match op {
            Op::Tool(i) => self.hashes[i],
            Op::Develop => mix(&("develop", DEVELOP_VERSION)),
            Op::Resize(w, h) => mix(&("resize", w, h)),
        }
    }

    fn run(&self, op: Op, input: &Image, source: &Source, input_key: u64) -> Result<Option<Image>> {
        match op {
            Op::Develop => {
                let raw = input.as_raw().ok_or_else(|| Error::Tool {
                    tool: "develop",
                    message: format!("was handed {}", input.describe()),
                })?;
                develop::develop(raw).map(|rgb| Some(Image::Rgb(rgb)))
            }
            Op::Resize(w, h) => {
                let rgb = input.as_rgb().ok_or_else(|| Error::Tool {
                    tool: "resize",
                    message: format!("was handed {}", input.describe()),
                })?;
                Ok(develop::resize(rgb, w, h).map(Image::Rgb))
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
                let cx = Context::for_tool(info.id, info.kind, source, input_key, &self.gpu);
                self.registry.tools[i].process(self.settings[i].as_ref(), input, &cx)
            }
        }
    }
}

fn mix(value: &impl Hash) -> u64 {
    let mut h = DefaultHasher::new();
    value.hash(&mut h);
    h.finish()
}

fn key_of(bytes: &[u8]) -> u64 {
    mix(&bytes)
}

/// A tool's settings, as a cache key: its id, its settings version and the
/// settings as JSON.
fn settings_hash(id: &str, version: u32, settings: &dyn AnySettings) -> u64 {
    mix(&(id, version, settings.to_json().to_string()))
}

/// Step results by key, least recently used first out.
struct Cache {
    entries: HashMap<u64, Entry>,
    budget: usize,
    used: usize,
    clock: u64,
}

struct Entry {
    image: Arc<Image>,
    /// What this entry adds to memory: nothing when it is a step that passed
    /// its input through, which another entry already counts.
    bytes: usize,
    used: u64,
}

impl Cache {
    fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            budget,
            used: 0,
            clock: 0,
        }
    }

    fn contains(&self, key: u64) -> bool {
        self.entries.contains_key(&key)
    }

    fn get(&mut self, key: u64) -> Option<Arc<Image>> {
        self.clock += 1;
        let entry = self.entries.get_mut(&key)?;
        entry.used = self.clock;
        Some(entry.image.clone())
    }

    fn put(&mut self, key: u64, image: Arc<Image>, fresh: bool) {
        self.clock += 1;
        let bytes = if fresh { image.bytes() } else { 0 };
        if let Some(old) = self.entries.insert(
            key,
            Entry {
                image,
                bytes,
                used: self.clock,
            },
        ) {
            self.used -= old.bytes;
        }
        self.used += bytes;
        self.evict();
    }

    fn evict(&mut self) {
        while self.used > self.budget && self.entries.len() > 1 {
            let Some((&oldest, _)) = self.entries.iter().min_by_key(|(_, e)| e.used) else {
                return;
            };
            if let Some(gone) = self.entries.remove(&oldest) {
                self.used -= gone.bytes;
            }
        }
    }
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
            "ag_exposure_wb": { "exposure": 1.5, "red": 1.2 },
        })))
        .unwrap();
        let doc = p.edit();
        let wb = &doc.tools["ag_exposure_wb"].settings;
        assert_eq!(
            (wb["exposure"].as_f64(), wb["red"].as_f64()),
            (Some(1.5), Some(1.2))
        );
        p.apply(&patch(
            serde_json::json!({ "ag_exposure_wb": { "exposure": null } }),
        ))
        .unwrap();
        let wb = &p.edit().tools["ag_exposure_wb"].settings;
        assert_eq!(wb["exposure"].as_f64(), Some(0.0), "back to its default");
        assert_eq!(wb["red"].as_f64(), Some(1.2), "the rest kept");
    }

    #[test]
    fn a_bad_patch_changes_nothing() {
        let mut p = Pipeline::standard().unwrap();
        let before = p.edit();
        for bad in [
            serde_json::json!({ "ag_nothing": { "x": 1 } }),
            serde_json::json!({ "ag_exposure_wb": { "contrast": 1 } }),
            serde_json::json!({ "ag_exposure_wb": { "exposure": "bright" } }),
            serde_json::json!({ "rt_capture_sharpen": { "iterations": 2.5 } }),
            // The first tool is fine, the second is not: neither changes.
            serde_json::json!({ "ag_exposure_wb": { "exposure": 1 }, "dt_highlights": { "recover": 3 } }),
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
