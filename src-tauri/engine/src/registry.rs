//! The tools, in the order they run.
//!
//! The list at the bottom is the whole registry: one line per tool, naming
//! its file and its type. That line declares the module and registers the
//! tool, so adding a tool touches this list and nothing else. The pipeline's
//! order comes from here, and so do the interface's panels.

use std::any::TypeId;

use serde_json::Value;

use crate::contract::{EDIT_VERSION, EditDocument, EditPatch, Origin, ToolDescriptor, ToolEdit};
use crate::tool::{self, AnySettings, AnyTool, Registered, Tool, ToolInfo};
use crate::{Error, Result, Source};

/// An ordered set of tools.
pub struct Registry {
    pub(crate) tools: Vec<Box<dyn AnyTool>>,
    infos: Vec<ToolInfo>,
    descriptors: Vec<std::result::Result<ToolDescriptor, String>>,
    types: Vec<TypeId>,
}

impl Default for Registry {
    fn default() -> Self {
        Self::new()
    }
}

impl Registry {
    /// No tools at all.
    pub fn new() -> Self {
        Self {
            tools: Vec::new(),
            infos: Vec::new(),
            descriptors: Vec::new(),
            types: Vec::new(),
        }
    }

    /// `T` after every tool already listed.
    pub fn with<T: Tool>(mut self) -> Self {
        // An id with no known origin prefix does not compile.
        const {
            assert!(
                Origin::of(T::ID).is_some(),
                "a tool id is an origin code (Origin::ALL), an underscore and a lowercase name"
            )
        };
        let tool = Registered(T::default());
        self.infos.push(tool.info());
        self.descriptors
            .push(tool.descriptor(self.tools.len() as u32));
        self.types.push(TypeId::of::<T>());
        self.tools.push(Box::new(tool));
        self
    }

    /// Every tool as the interface builds its panel, in pipeline order.
    pub fn descriptors(&self) -> Vec<ToolDescriptor> {
        self.descriptors
            .iter()
            .filter_map(|d| d.as_ref().ok().cloned())
            .collect()
    }

    pub(crate) fn infos(&self) -> &[ToolInfo] {
        &self.infos
    }

    pub fn position(&self, id: &str) -> Option<usize> {
        self.infos.iter().position(|t| t.id == id)
    }

    pub(crate) fn position_of<T: Tool>(&self) -> Option<usize> {
        self.types.iter().position(|t| *t == TypeId::of::<T>())
    }

    /// `patch` merged into `doc`. `null` puts a setting back to its value in
    /// `defaults`: the photo's own, from `defaults_for`, or `default_edit`.
    /// The result is every tool's settings as the engine reads them:
    /// migrated, clamped, the defaults filled in. All or nothing: an unknown
    /// tool or setting, or a value of the wrong type, refuses the whole patch.
    pub fn merge(
        &self,
        doc: &EditDocument,
        patch: &EditPatch,
        defaults: &EditDocument,
    ) -> Result<EditDocument> {
        let mut settings = self.read_edit(doc)?;
        for (id, changes) in &patch.0 {
            let i = self
                .position(id)
                .ok_or_else(|| Error::UnknownTool(id.clone()))?;
            let mut merged = tool::as_map(settings[i].to_json());
            let reset = defaults.tools.get(id).map(|t| &t.settings);
            for (key, value) in changes {
                if !merged.contains_key(key) {
                    return Err(Error::UnknownSetting {
                        tool: id.clone(),
                        key: key.clone(),
                    });
                }
                if !value.is_null() {
                    merged.insert(key.clone(), value.clone());
                } else if let Some(default) = reset.and_then(|r| r.get(key)) {
                    merged.insert(key.clone(), default.clone());
                } else {
                    // Read back as the settings' own default.
                    merged.remove(key);
                }
            }
            let value = Value::Object(merged.into_iter().collect());
            settings[i] = self.tools[i].read(self.infos[i].version, value)?;
        }
        Ok(self.edit_of(&settings))
    }

    /// Every tool at its defaults, as an edit: what the descriptors say.
    pub fn default_edit(&self) -> EditDocument {
        let defaults: Vec<_> = self.tools.iter().map(|t| t.defaults()).collect();
        self.edit_of(&defaults)
    }

    /// Every tool at the defaults it gives this photo (`Tool::defaults_for`),
    /// as an edit: what a photo opens with.
    pub fn defaults_for(&self, source: &Source) -> EditDocument {
        let defaults: Vec<_> = self.tools.iter().map(|t| t.defaults_for(source)).collect();
        self.edit_of(&defaults)
    }

    /// Every tool's settings from an edit, in registry order; a tool the
    /// edit does not mention is at its defaults.
    pub(crate) fn read_edit(&self, doc: &EditDocument) -> Result<Vec<Box<dyn AnySettings>>> {
        if doc.version != EDIT_VERSION {
            return Err(Error::BadEdit(format!(
                "edit version {}, this engine reads {EDIT_VERSION}",
                doc.version
            )));
        }
        if let Some(id) = doc.tools.keys().find(|id| self.position(id).is_none()) {
            return Err(Error::UnknownTool(id.clone()));
        }
        self.infos
            .iter()
            .enumerate()
            .map(|(i, info)| match doc.tools.get(info.id) {
                Some(edit) => {
                    let value = Value::Object(edit.settings.clone().into_iter().collect());
                    self.tools[i].read(edit.v, value)
                }
                None => Ok(self.tools[i].defaults()),
            })
            .collect()
    }

    /// Settings in registry order, as an edit saves them.
    pub(crate) fn edit_of(&self, settings: &[Box<dyn AnySettings>]) -> EditDocument {
        let mut doc = EditDocument::new();
        for (info, settings) in self.infos.iter().zip(settings) {
            doc.tools.insert(
                info.id.to_string(),
                ToolEdit {
                    v: info.version,
                    settings: tool::as_map(settings.to_json()),
                },
            );
        }
        doc
    }

    /// Ids are unique, stages come in order, and every tool's settings can
    /// be described to the interface.
    pub fn check(&self) -> Result<()> {
        for (i, tool) in self.infos.iter().enumerate() {
            if self.infos[..i].iter().any(|t| t.id == tool.id) {
                return Err(Error::Registry(format!("{} is registered twice", tool.id)));
            }
            if let Some(before) = self.infos[..i].iter().find(|t| t.stage > tool.stage) {
                return Err(Error::Registry(format!(
                    "{} runs at {:?} and is listed after {}, which runs at {:?}: \
                     list every Raw tool first, then SceneLinear, then Display",
                    tool.id, tool.stage, before.id, before.stage
                )));
            }
            if tool.pixel.is_some() && tool.stage != crate::contract::Stage::SceneLinear {
                return Err(Error::Registry(format!(
                    "{} is a per-pixel tool at {:?}: the pixel pass runs on linear RGB",
                    tool.id, tool.stage
                )));
            }
            if let Err(e) = &self.descriptors[i] {
                return Err(Error::Registry(e.clone()));
            }
        }
        Ok(())
    }
}

/// Declares each tool's module and lists the tools, in order.
macro_rules! registry {
    ($($file:literal => $module:ident :: $tool:ident,)*) => {
        $(
            #[path = $file]
            pub mod $module;
        )*

        /// Every tool's module, by its id.
        pub mod tools {
            $(pub use super::$module;)*
        }

        /// The tools Argentum ships, in pipeline order.
        pub fn standard() -> Registry {
            Registry::new()$(.with::<$module::$tool>())*
        }
    };
}

registry! {
    "tools/dt_highlights.rs"      => dt_highlights::DtHighlights,
    "tools/rt_capture_sharpen/mod.rs" => rt_capture_sharpen::RtCaptureSharpen,
    "tools/ag_exposure.rs"        => ag_exposure::AgExposure,
    "tools/rr_white_balance.rs"   => rr_white_balance::RrWhiteBalance,
    "tools/rr_basic_tone/mod.rs"  => rr_basic_tone::RrBasicTone,
    "tools/rr_presence.rs"        => rr_presence::RrPresence,
    "tools/ag_dng_standard.rs"    => ag_dng_standard::AgDngStandard,
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::contract::Stage;

    #[test]
    fn the_standard_registry_is_consistent() {
        let registry = standard();
        registry.check().expect("valid");
        let ids: Vec<_> = registry.infos().iter().map(|t| t.id).collect();
        assert_eq!(
            ids,
            [
                "dt_highlights",
                "rt_capture_sharpen",
                "ag_exposure",
                "rr_white_balance",
                "rr_basic_tone",
                "rr_presence",
                "ag_dng_standard",
            ]
        );
        let stages: Vec<_> = registry.infos().iter().map(|t| t.stage).collect();
        assert_eq!(stages[0], Stage::Raw);
        assert!(stages[1..].iter().all(|s| *s == Stage::SceneLinear));
        let orders: Vec<_> = registry.descriptors().iter().map(|d| d.order).collect();
        assert_eq!(orders, [0, 1, 2, 3, 4, 5, 6]);
    }

    #[test]
    fn a_tool_listed_twice_is_refused() {
        let twice = standard().with::<ag_exposure::AgExposure>();
        assert!(matches!(twice.check(), Err(Error::Registry(_))));
    }

    #[test]
    fn a_raw_tool_after_a_scene_tool_is_refused() {
        let out_of_order = Registry::new()
            .with::<ag_exposure::AgExposure>()
            .with::<dt_highlights::DtHighlights>();
        let err = out_of_order.check().unwrap_err().to_string();
        assert!(err.contains("dt_highlights"), "{err}");
    }
}
