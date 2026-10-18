//! The contract every tool implements.
//!
//! A tool is a type with a stable id, the stage it runs in, the section its
//! panel goes in, whether it needs the GPU, a settings struct, and one
//! function from an image to an image. That is all of it. The registry puts
//! tools in order, the pipeline runs and caches them, and the interface
//! builds each panel from a descriptor read off the tool itself; none of them
//! knows anything about a particular tool, which is what lets a new one
//! arrive as one file and one registry line.

use std::any::Any;
use std::collections::BTreeMap;
use std::fmt::Debug;
use std::sync::Arc;

use serde::Serialize;
use serde::de::DeserializeOwned;
use serde_json::Value;

use crate::contract::{
    Choice, EnabledWhen, Origin, Section, SettingKind, SettingScale, SettingSpec, Stage,
    ToolDescriptor,
};
use crate::{Error, Gpu, GpuSlot, Image, Result, Source};

/// What a panel needs to know about one setting beyond its type and default,
/// which are read off the settings struct itself. Numbers outside
/// `min..max` are clamped whenever settings are read or set, so a stale or
/// hand-edited edit can never hand a tool something out of range.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Field {
    /// The setting's name in the settings' JSON.
    pub key: &'static str,
    pub min: Option<f64>,
    pub max: Option<f64>,
    pub step: Option<f64>,
    pub unit: Option<&'static str>,
    /// For a string setting: the values it may take.
    pub choices: &'static [&'static str],
    /// For a float: its control moves by ratio rather than evenly.
    pub log: bool,
    /// Shown only while another setting of the tool has this value.
    pub enabled_when: Option<(&'static str, Equals)>,
    pub advanced: bool,
}

/// A value another setting is compared with, in `Field::enabled_when`.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum Equals {
    Bool(bool),
    Number(f64),
    Choice(&'static str),
}

impl Field {
    pub const fn new(key: &'static str) -> Self {
        Self {
            key,
            min: None,
            max: None,
            step: None,
            unit: None,
            choices: &[],
            log: false,
            enabled_when: None,
            advanced: false,
        }
    }

    pub const fn range(self, min: f64, max: f64) -> Self {
        Self {
            min: Some(min),
            max: Some(max),
            ..self
        }
    }

    pub const fn step(self, step: f64) -> Self {
        Self {
            step: Some(step),
            ..self
        }
    }

    pub const fn unit(self, unit: &'static str) -> Self {
        Self {
            unit: Some(unit),
            ..self
        }
    }

    pub const fn choices(self, choices: &'static [&'static str]) -> Self {
        Self { choices, ..self }
    }

    pub const fn log(self) -> Self {
        Self { log: true, ..self }
    }

    pub const fn enabled_when(self, key: &'static str, equals: Equals) -> Self {
        Self {
            enabled_when: Some((key, equals)),
            ..self
        }
    }

    pub const fn advanced(self) -> Self {
        Self {
            advanced: true,
            ..self
        }
    }
}

/// A tool's settings: what is saved in an edit and hashed into the cache key.
///
/// Give the struct `#[serde(default)]`, so a saved edit missing a field reads
/// that field's default rather than failing. Each field is a number (a float
/// or an integer), a boolean, or a string that is one of its `choices`.
pub trait Settings:
    Serialize + DeserializeOwned + Default + Clone + PartialEq + Debug + Send + Sync + 'static
{
    /// Raised whenever a saved value would mean something different. Edits
    /// store it beside the settings, and older ones go through `migrate`.
    const VERSION: u32;

    /// What a panel needs beyond each setting's type and default.
    const FIELDS: &'static [Field] = &[];

    /// Turn settings saved at version `from`, older than `VERSION`, into this
    /// version's JSON. The default knows no older version.
    fn migrate(from: u32, value: Value) -> std::result::Result<Value, String> {
        let _ = value;
        Err(format!("no migration from version {from}"))
    }
}

/// A per-pixel tool's part of the pixel pass: what it does to one pixel,
/// in WGSL, and how many parameters it reads.
///
/// The pipeline runs each run of consecutive per-pixel tools as one GPU
/// dispatch, calling them in registry order from a `main` it writes itself,
/// as the app's `shader.wgsl` calls its adjustments one after another. A
/// tool's WGSL defines
///
/// ```wgsl
/// fn <tool id>(c: vec3<f32>, px: Px) -> vec3<f32>
/// ```
///
/// which takes the colour so far and returns the next. `Px` gives the
/// pixel's place in the pass's input (`coord`), its colour as the pass
/// received it (`input`), and where the tool's parameters start in
/// `ag_params` (`p`). The pass also provides `ag_input` (the input picture),
/// `ag_gf_tonal` and `ag_gf_coarse` (its guided filter, for a tool with
/// `detail`), and the helpers in `src/pixels/common.wgsl`. Other functions
/// the tool needs carry its id as a prefix, or the name of the app's
/// function they were copied from, which the app's one shader keeps unique.
/// Given parameters of all zeros, a tool leaves the colour alone.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pixel {
    pub wgsl: &'static str,
    /// How many `vec4<f32>` it reads, from `ag_params[px.p]` on.
    pub params: usize,
    /// Whether it reads the guided filter of the pass's input.
    pub detail: bool,
}

/// One tool.
pub trait Tool: Default + Send + Sync + 'static {
    /// Stable for ever once an edit has saved it: origin prefix and a name,
    /// lowercase, as `Origin::of` describes.
    const ID: &'static str;
    const STAGE: Stage;
    const SECTION: Section;
    /// Whether it runs on the GPU. Only such a tool is handed the device by
    /// `Context::gpu`.
    const GPU: bool = false;
    /// Set for a tool that changes each pixel on its own: it runs in the
    /// pixel pass, never in `process`.
    const PIXEL: Option<Pixel> = None;
    type Settings: Settings;

    /// The settings `source` starts with: the settings' `Default`, unless
    /// the starting point is the photo's own - the camera's white balance, a
    /// body's exposure offset. An edit stores them like any other value, and
    /// `null` in a patch puts them back. darktable's `reload_defaults`, which
    /// its white balance and exposure modules use for the same reason.
    fn defaults_for(&self, source: &Source) -> Self::Settings {
        let _ = source;
        Self::Settings::default()
    }

    /// A whole picture in, a new one out. `Ok(None)` means there is nothing
    /// to do: the input is the output, and the pipeline keeps the same
    /// picture rather than a copy of it. A per-pixel tool has none.
    fn process(
        &self,
        settings: &Self::Settings,
        input: &Image,
        cx: &Context,
    ) -> Result<Option<Image>> {
        let _ = (settings, input, cx);
        Err(Error::Tool {
            tool: Self::ID,
            message: "runs in the pixel pass, not on whole pictures".into(),
        })
    }

    /// A per-pixel tool's parameters for one pass, `PIXEL`'s `params` of
    /// them: `None` when its settings change nothing, and the pass then
    /// hands it zeros.
    fn pixel_params(&self, settings: &Self::Settings, cx: &Context) -> Option<Vec<[f32; 4]>> {
        let _ = (settings, cx);
        None
    }
}

/// What a tool can see besides its input.
pub struct Context<'a> {
    /// The photo as rawler decoded it, before any tool touched it - for a
    /// measurement that needs the sensor's own data, such as capture
    /// sharpening's radius.
    pub source: &'a Source,
    /// Names the input exactly: the same key is the same pixels. For a tool
    /// that keeps a measurement between runs.
    pub input_key: u64,
    tool: &'static str,
    gpu_allowed: bool,
    gpu: &'a GpuSlot,
}

impl<'a> Context<'a> {
    /// The context `T` runs in. The pipeline makes these; so can a caller
    /// running one tool on its own.
    pub fn new<T: Tool>(source: &'a Source, input_key: u64, gpu: &'a GpuSlot) -> Self {
        Self::for_tool(T::ID, T::GPU, source, input_key, gpu)
    }

    pub(crate) fn for_tool(
        tool: &'static str,
        gpu_allowed: bool,
        source: &'a Source,
        input_key: u64,
        gpu: &'a GpuSlot,
    ) -> Self {
        Self {
            source,
            input_key,
            tool,
            gpu_allowed,
            gpu,
        }
    }

    /// The engine's GPU device, opened the first time any tool asks.
    pub fn gpu(&self) -> Result<Arc<Gpu>> {
        if !self.gpu_allowed {
            return Err(Error::Tool {
                tool: self.tool,
                message: "only a tool with GPU = true is given the GPU".into(),
            });
        }
        self.gpu.get()
    }
}

/// Why settings could not be read.
#[derive(Debug, Clone, PartialEq)]
pub enum Refused {
    /// Saved by a newer engine, or by a version with no migration.
    Version(String),
    /// A value of the wrong type, or not one of its choices.
    Value(String),
}

/// Read settings saved at `version`: migrated if older, refused if newer,
/// then clamped to their fields' ranges.
pub fn read_settings<S: Settings>(version: u32, value: Value) -> std::result::Result<S, Refused> {
    let value = match version.cmp(&S::VERSION) {
        std::cmp::Ordering::Equal => value,
        std::cmp::Ordering::Less => S::migrate(version, value).map_err(Refused::Version)?,
        std::cmp::Ordering::Greater => {
            return Err(Refused::Version(format!(
                "saved at version {version}, newer than this engine's {}",
                S::VERSION
            )));
        }
    };
    let (value, _) = clamp_json(value, S::FIELDS).map_err(Refused::Value)?;
    serde_json::from_value(value).map_err(|e| Refused::Value(e.to_string()))
}

/// The same settings with every value inside its range.
pub fn clamp<S: Settings>(settings: S) -> S {
    let Ok(value) = serde_json::to_value(&settings) else {
        return settings;
    };
    match clamp_json(value, S::FIELDS) {
        Ok((value, true)) => serde_json::from_value(value).unwrap_or(settings),
        _ => settings,
    }
}

/// Clamp the fields that have a range, and refuse a choice that is not one.
/// Says whether anything moved, so settings already in range are kept
/// exactly as they were.
fn clamp_json(mut value: Value, fields: &[Field]) -> std::result::Result<(Value, bool), String> {
    let mut moved = false;
    if let Value::Object(map) = &mut value {
        for field in fields {
            let Some(v) = map.get_mut(field.key) else {
                continue;
            };
            if !field.choices.is_empty() {
                if !v.as_str().is_some_and(|s| field.choices.contains(&s)) {
                    return Err(format!(
                        "{} is {v}, not one of {:?}",
                        field.key, field.choices
                    ));
                }
                continue;
            }
            let (Some(min), Some(max)) = (field.min, field.max) else {
                continue;
            };
            let Value::Number(n) = &*v else { continue };
            // Integers stay integers, or a u32 field would not read back.
            let next = if let Some(i) = n.as_i64() {
                let c = i.clamp(min.ceil() as i64, max.floor() as i64);
                (c != i).then(|| Value::from(c))
            } else if n.as_u64().is_some() {
                Some(Value::from(max.floor() as i64))
            } else {
                let f = n.as_f64().unwrap_or(0.0);
                let c = f.clamp(min, max);
                (c != f).then(|| Value::from(c))
            };
            if let Some(clamped) = next {
                *v = clamped;
                moved = true;
            }
        }
    }
    Ok((value, moved))
}

/// A tool's descriptor, read off its settings' defaults and fields. Refused
/// when a setting is not a number, a boolean or a choice, a field names a
/// setting that does not exist, a log scale is asked of something that is
/// not a float, or a setting depends on itself, on a setting that does not
/// exist, or on a value of the wrong type.
fn describe(
    tool: ToolDescriptor,
    defaults: &Value,
    fields: &[Field],
) -> std::result::Result<ToolDescriptor, String> {
    let id = &tool.id;
    let Value::Object(defaults) = defaults else {
        return Err(format!("{id}: settings are not a struct"));
    };
    if let Some(field) = fields.iter().find(|f| !defaults.contains_key(f.key)) {
        return Err(format!(
            "{id}: a field names {}, which is not a setting",
            field.key
        ));
    }
    let mut settings = Vec::with_capacity(defaults.len());
    // The order the struct declares them in: serde_json keeps keys in
    // insertion order only with its `preserve_order` feature, so the fields'
    // own order leads and the rest follow alphabetically.
    let mut keys: Vec<&String> = fields
        .iter()
        .filter_map(|f| defaults.get_key_value(f.key).map(|(k, _)| k))
        .collect();
    keys.extend(
        defaults
            .keys()
            .filter(|k| !fields.iter().any(|f| f.key == *k)),
    );
    for key in keys {
        let default = &defaults[key];
        let field = fields.iter().find(|f| f.key == key);
        let kind = match default {
            Value::Bool(_) => SettingKind::Bool,
            Value::Number(n) if n.is_f64() => SettingKind::Float,
            Value::Number(_) => SettingKind::Int,
            Value::String(_) if field.is_some_and(|f| !f.choices.is_empty()) => SettingKind::Choice,
            _ => {
                return Err(format!(
                    "{id}: {key} is not a number, a boolean or a choice with its choices declared"
                ));
            }
        };
        let log = field.is_some_and(|f| f.log);
        if log && kind != SettingKind::Float {
            return Err(format!("{id}: {key} is not a float, so has no log scale"));
        }
        let enabled_when = match field.and_then(|f| f.enabled_when) {
            None => None,
            Some((other, equals)) => {
                let target = defaults.get(other).filter(|_| other != key.as_str());
                let equals = match (equals, target) {
                    (Equals::Bool(b), Some(Value::Bool(_))) => Value::Bool(b),
                    (Equals::Number(n), Some(Value::Number(_))) => Value::from(n),
                    (Equals::Choice(c), Some(Value::String(_))) => Value::from(c),
                    _ => {
                        return Err(format!(
                            "{id}: {key} depends on {other}, which is not another setting of that type"
                        ));
                    }
                };
                Some(EnabledWhen {
                    key: other.to_string(),
                    equals,
                })
            }
        };
        let label = format!("tools.{id}.{key}");
        settings.push(SettingSpec {
            key: key.clone(),
            kind,
            default: default.clone(),
            min: field.and_then(|f| f.min),
            max: field.and_then(|f| f.max),
            step: field
                .and_then(|f| f.step)
                .or((kind == SettingKind::Int).then_some(1.0)),
            unit: field.and_then(|f| f.unit).map(str::to_string),
            scale: (kind == SettingKind::Float).then_some(if log {
                SettingScale::Log
            } else {
                SettingScale::Linear
            }),
            enabled_when,
            choices: field.filter(|f| !f.choices.is_empty()).map(|f| {
                f.choices
                    .iter()
                    .map(|c| Choice {
                        value: c.to_string(),
                        label: format!("{label}.{c}"),
                    })
                    .collect()
            }),
            advanced: field.is_some_and(|f| f.advanced),
            label,
        });
    }
    Ok(ToolDescriptor { settings, ..tool })
}

/// A tool's descriptor with no settings yet.
fn skeleton(
    id: &str,
    stage: Stage,
    section: Section,
    version: u32,
    order: u32,
) -> std::result::Result<ToolDescriptor, String> {
    Ok(ToolDescriptor {
        id: id.to_string(),
        origin: Origin::of(id).ok_or_else(|| format!("{id} has no origin prefix"))?,
        label: format!("tools.{id}.label"),
        section,
        order,
        stage,
        version,
        settings: Vec::new(),
    })
}

/// What the engine needs to know about a registered tool, without its type.
#[derive(Debug, Clone, PartialEq)]
pub(crate) struct ToolInfo {
    pub id: &'static str,
    pub stage: Stage,
    pub gpu: bool,
    pub pixel: Option<Pixel>,
    pub version: u32,
}

// ---------------------------------------------------------------------------
// The same contract with the types erased, so one registry can hold them all.
// ---------------------------------------------------------------------------

pub(crate) trait AnySettings: Debug + Send + Sync {
    fn as_any(&self) -> &dyn Any;
    fn to_json(&self) -> Value;
}

impl<S: Settings> AnySettings for S {
    fn as_any(&self) -> &dyn Any {
        self
    }

    /// Through text rather than `to_value`, which widens an f32 to the f64
    /// nearest it: 1.2 would read back as 1.2000000476837158.
    fn to_json(&self) -> Value {
        serde_json::to_string(self)
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null)
    }
}

pub(crate) trait AnyTool: Send + Sync {
    fn info(&self) -> ToolInfo;
    fn descriptor(&self, order: u32) -> std::result::Result<ToolDescriptor, String>;
    fn defaults(&self) -> Box<dyn AnySettings>;
    fn defaults_for(&self, source: &Source) -> Box<dyn AnySettings>;
    fn read(&self, version: u32, value: Value) -> Result<Box<dyn AnySettings>>;
    fn process(
        &self,
        settings: &dyn AnySettings,
        input: &Image,
        cx: &Context,
    ) -> Result<Option<Image>>;
    fn pixel_params(
        &self,
        settings: &dyn AnySettings,
        cx: &Context,
    ) -> Result<Option<Vec<[f32; 4]>>>;
}

pub(crate) struct Registered<T: Tool>(pub T);

impl<T: Tool> Registered<T> {
    fn settings<'a>(&self, settings: &'a dyn AnySettings) -> Result<&'a T::Settings> {
        settings
            .as_any()
            .downcast_ref::<T::Settings>()
            .ok_or_else(|| Error::Registry(format!("{} was handed another tool's settings", T::ID)))
    }
}

impl<T: Tool> AnyTool for Registered<T> {
    fn info(&self) -> ToolInfo {
        ToolInfo {
            id: T::ID,
            stage: T::STAGE,
            gpu: T::GPU,
            pixel: T::PIXEL,
            version: <T::Settings as Settings>::VERSION,
        }
    }

    fn descriptor(&self, order: u32) -> std::result::Result<ToolDescriptor, String> {
        describe(
            skeleton(
                T::ID,
                T::STAGE,
                T::SECTION,
                <T::Settings as Settings>::VERSION,
                order,
            )?,
            &T::Settings::default().to_json(),
            <T::Settings as Settings>::FIELDS,
        )
    }

    fn defaults(&self) -> Box<dyn AnySettings> {
        Box::new(T::Settings::default())
    }

    fn defaults_for(&self, source: &Source) -> Box<dyn AnySettings> {
        Box::new(clamp(self.0.defaults_for(source)))
    }

    fn read(&self, version: u32, value: Value) -> Result<Box<dyn AnySettings>> {
        match read_settings::<T::Settings>(version, value) {
            Ok(settings) => Ok(Box::new(settings)),
            Err(Refused::Version(m)) => Err(Error::BadEdit(format!("{}: {m}", T::ID))),
            Err(Refused::Value(message)) => Err(Error::BadValue {
                tool: T::ID.to_string(),
                message,
            }),
        }
    }

    fn process(
        &self,
        settings: &dyn AnySettings,
        input: &Image,
        cx: &Context,
    ) -> Result<Option<Image>> {
        self.0.process(self.settings(settings)?, input, cx)
    }

    fn pixel_params(
        &self,
        settings: &dyn AnySettings,
        cx: &Context,
    ) -> Result<Option<Vec<[f32; 4]>>> {
        let params = self.0.pixel_params(self.settings(settings)?, cx);
        let wanted = T::PIXEL.map_or(0, |p| p.params);
        match params {
            Some(p) if p.len() != wanted => Err(Error::Tool {
                tool: T::ID,
                message: format!("gave {} parameters to a pass that reads {wanted}", p.len()),
            }),
            params => Ok(params),
        }
    }
}

/// Settings as JSON, field by field.
pub(crate) fn as_map(value: Value) -> BTreeMap<String, Value> {
    match value {
        Value::Object(map) => map.into_iter().collect(),
        _ => BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde::Deserialize;

    #[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
    #[serde(default)]
    struct Demo {
        amount: f32,
        count: u32,
        on: bool,
        mode: String,
    }

    impl Default for Demo {
        fn default() -> Self {
            Self {
                amount: 0.5,
                count: 3,
                on: true,
                mode: "soft".into(),
            }
        }
    }

    impl Settings for Demo {
        const VERSION: u32 = 2;
        const FIELDS: &'static [Field] = &[
            Field::new("amount").range(0.0, 1.0).step(0.01).unit("%"),
            Field::new("count")
                .range(1.0, 10.0)
                .advanced()
                .enabled_when("on", Equals::Bool(true)),
            Field::new("mode").choices(&["soft", "hard"]),
        ];

        fn migrate(from: u32, mut value: Value) -> std::result::Result<Value, String> {
            if from != 1 {
                return Err(format!("no migration from version {from}"));
            }
            // Version 1 stored a percentage.
            if let Some(pct) = value.get("amount").and_then(Value::as_f64) {
                value["amount"] = Value::from(pct / 100.0);
            }
            Ok(value)
        }
    }

    #[test]
    fn values_out_of_range_are_clamped_and_types_kept() {
        let s: Demo = read_settings(2, serde_json::json!({ "amount": 7.0, "count": 0 })).unwrap();
        assert_eq!((s.amount, s.count), (1.0, 1));
    }

    #[test]
    fn a_choice_that_is_not_one_is_refused() {
        assert!(read_settings::<Demo>(2, serde_json::json!({ "mode": "medium" })).is_err());
        assert!(read_settings::<Demo>(2, serde_json::json!({ "mode": "hard" })).is_ok());
    }

    #[test]
    fn missing_fields_read_as_defaults() {
        let s: Demo = read_settings(2, serde_json::json!({ "count": 5 })).unwrap();
        assert_eq!(
            s,
            Demo {
                count: 5,
                ..Demo::default()
            }
        );
    }

    #[test]
    fn older_versions_migrate_and_newer_ones_are_refused() {
        let s: Demo = read_settings(1, serde_json::json!({ "amount": 25.0 })).unwrap();
        assert_eq!(s.amount, 0.25);
        assert!(read_settings::<Demo>(3, serde_json::json!({})).is_err());
        assert!(read_settings::<Demo>(0, serde_json::json!({})).is_err());
    }

    #[test]
    fn settings_in_range_are_kept_bit_for_bit() {
        let s = Demo {
            amount: 0.1 + 0.2,
            ..Demo::default()
        };
        assert_eq!(clamp(s.clone()), s);
        assert_eq!(
            clamp(Demo {
                amount: -1.0,
                ..s.clone()
            })
            .amount,
            0.0
        );
    }

    #[test]
    fn the_descriptor_is_read_off_the_settings() {
        let tool = skeleton(
            "ag_demo",
            Stage::SceneLinear,
            Section::Basic,
            Demo::VERSION,
            7,
        );
        let d = describe(tool.unwrap(), &Demo::default().to_json(), Demo::FIELDS).unwrap();
        assert_eq!(d.label, "tools.ag_demo.label");
        assert_eq!(d.origin, Origin::Argentum);
        assert_eq!(d.order, 7);
        let keys: Vec<_> = d.settings.iter().map(|s| s.key.as_str()).collect();
        assert_eq!(keys, ["amount", "count", "mode", "on"]);
        let [amount, count, mode, on] = &d.settings[..] else {
            panic!()
        };
        assert_eq!(amount.kind, SettingKind::Float);
        assert_eq!(
            (amount.min, amount.max, amount.step),
            (Some(0.0), Some(1.0), Some(0.01))
        );
        assert_eq!(amount.unit.as_deref(), Some("%"));
        assert_eq!(amount.label, "tools.ag_demo.amount");
        assert_eq!(
            (count.kind, count.step, count.advanced),
            (SettingKind::Int, Some(1.0), true)
        );
        assert_eq!(
            count.enabled_when,
            Some(EnabledWhen {
                key: "on".into(),
                equals: Value::Bool(true)
            })
        );
        assert_eq!(
            (amount.scale, count.scale),
            (Some(SettingScale::Linear), None)
        );
        assert_eq!(mode.kind, SettingKind::Choice);
        assert_eq!(
            mode.choices.as_ref().unwrap()[1],
            Choice {
                value: "hard".into(),
                label: "tools.ag_demo.mode.hard".into()
            }
        );
        assert_eq!(
            (on.kind, &on.default),
            (SettingKind::Bool, &Value::Bool(true))
        );
    }

    #[test]
    fn a_setting_the_contract_cannot_show_is_refused() {
        let tool = || skeleton("ag_demo", Stage::SceneLinear, Section::Basic, 1, 0).unwrap();
        let arrays = serde_json::json!({ "gains": [1.0, 1.0] });
        assert!(describe(tool(), &arrays, &[]).is_err());
        let missing = [Field::new("nothing")];
        assert!(describe(tool(), &Demo::default().to_json(), &missing).is_err());
        let defaults = Demo::default().to_json();
        // Its choices declared, so `mode` is not what each case is refused for.
        let mode = Field::new("mode").choices(&["soft", "hard"]);
        assert!(describe(tool(), &defaults, &[mode]).is_ok());
        for wrong in [
            // A log scale on an integer.
            Field::new("count").log(),
            // Depending on itself, on nothing, or on a value of another type.
            Field::new("count").enabled_when("count", Equals::Number(1.0)),
            Field::new("count").enabled_when("nothing", Equals::Bool(true)),
            Field::new("count").enabled_when("on", Equals::Choice("soft")),
        ] {
            assert!(
                describe(tool(), &defaults, &[wrong, mode]).is_err(),
                "{wrong:?}"
            );
        }
        let log = describe(tool(), &defaults, &[Field::new("amount").log(), mode]).unwrap();
        assert_eq!(log.settings[0].scale, Some(SettingScale::Log));
    }
}
