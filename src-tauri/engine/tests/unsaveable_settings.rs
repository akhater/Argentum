//! Settings that cannot be written as JSON are an error at the edge, never
//! an empty entry in the edit or a shared cache key.

#[path = "support/ag_dummy.rs"]
mod ag_dummy;

use ag_dummy::AgDummy;
use argentum_engine::contract::{EditPatch, ErrorKind};
use argentum_engine::{
    Context, Error, Field, Image, Pipeline, Result, Section, Stage, Tool, standard,
};
use serde::{Deserialize, Serialize, Serializer};

/// Settings whose `Serialize` refuses once `poison` is set: its defaults are
/// fine, so the registry accepts it.
#[derive(Debug, Clone, PartialEq, Deserialize)]
#[serde(default)]
pub struct Poisonable {
    amount: f32,
    poison: bool,
}

impl Default for Poisonable {
    fn default() -> Self {
        Self {
            amount: 0.0,
            poison: false,
        }
    }
}

impl Serialize for Poisonable {
    fn serialize<S: Serializer>(&self, s: S) -> std::result::Result<S::Ok, S::Error> {
        use serde::ser::{Error as _, SerializeStruct};
        if self.poison {
            return Err(S::Error::custom("poisoned"));
        }
        let mut out = s.serialize_struct("Poisonable", 2)?;
        out.serialize_field("amount", &self.amount)?;
        out.serialize_field("poison", &self.poison)?;
        out.end()
    }
}

impl argentum_engine::Settings for Poisonable {
    const VERSION: u32 = 1;
    const FIELDS: &'static [Field] = &[Field::new("amount").range(0.0, 1.0)];
}

#[derive(Default)]
struct AgPoison;

impl Tool for AgPoison {
    const ID: &'static str = "ag_poison";
    const STAGE: Stage = Stage::SceneLinear;
    const SECTION: Section = Section::Effects;
    type Settings = Poisonable;

    fn process(&self, _: &Poisonable, _: &Image, _: &Context) -> Result<Option<Image>> {
        Ok(None)
    }
}

fn patch(json: serde_json::Value) -> EditPatch {
    serde_json::from_value(json).unwrap()
}

fn pipeline() -> Pipeline {
    Pipeline::new(standard().with::<AgPoison>()).expect("its defaults serialise")
}

fn refused(e: Error, tool: &str) {
    assert_eq!(e.kind(), ErrorKind::BadValue, "{e}");
    assert!(e.to_string().contains(tool), "{e}");
}

#[test]
fn a_patch_that_cannot_be_saved_is_refused_and_changes_nothing() {
    let mut p = pipeline();
    let before = p.edit().unwrap();
    let e = p
        .apply(&patch(
            serde_json::json!({ "ag_poison": { "poison": true } }),
        ))
        .unwrap_err();
    refused(e, "ag_poison");
    assert_eq!(p.edit().unwrap(), before);
    assert!(!p.get::<AgPoison>().unwrap().poison);
}

#[test]
fn settings_set_directly_that_cannot_be_saved_are_refused() {
    let mut p = pipeline();
    let before = p.edit().unwrap();
    let bad = Poisonable {
        poison: true,
        ..Default::default()
    };
    refused(p.set::<AgPoison>(bad).unwrap_err(), "ag_poison");
    assert_eq!(p.edit().unwrap(), before);
}

#[test]
fn a_registry_whose_defaults_cannot_be_saved_does_not_start() {
    #[derive(Default)]
    struct AgBadDefault;
    #[derive(Debug, Clone, PartialEq, Deserialize)]
    struct Never;
    impl Default for Never {
        fn default() -> Self {
            Never
        }
    }
    impl Serialize for Never {
        fn serialize<S: Serializer>(&self, _: S) -> std::result::Result<S::Ok, S::Error> {
            Err(serde::ser::Error::custom("never"))
        }
    }
    impl argentum_engine::Settings for Never {
        const VERSION: u32 = 1;
    }
    impl Tool for AgBadDefault {
        const ID: &'static str = "ag_bad_default";
        const STAGE: Stage = Stage::SceneLinear;
        const SECTION: Section = Section::Effects;
        type Settings = Never;
    }
    assert!(Pipeline::new(standard().with::<AgBadDefault>()).is_err());
}

/// serde_json writes a NaN or an infinity as `null`. A slider cannot send
/// one (JSON has none), but code can: it is refused, naming the tool and the
/// field, and what was set before stays.
#[test]
fn a_non_finite_setting_is_refused_naming_the_tool_and_field() {
    for bad in [f32::NAN, f32::INFINITY, f32::NEG_INFINITY] {
        let mut p = Pipeline::new(standard().with::<AgDummy>()).unwrap();
        p.set::<AgDummy>(ag_dummy::Settings { amount: 0.5 })
            .unwrap();
        let before = p.edit().unwrap();
        let e = p
            .set::<AgDummy>(ag_dummy::Settings { amount: bad })
            .unwrap_err();
        refused(e.clone(), "ag_dummy");
        assert!(e.to_string().contains("amount"), "{e}");
        assert_eq!(p.edit().unwrap(), before, "{bad}");
        assert_eq!(p.get::<AgDummy>().unwrap().amount, 0.5, "{bad}");
    }
}

#[test]
fn every_registered_tools_defaults_round_trip() {
    let registry = standard().with::<AgDummy>().with::<AgPoison>();
    let mut p = Pipeline::new(registry).unwrap();
    let defaults = p.registry().default_edit().unwrap();
    assert_eq!(defaults.tools.len(), p.registry().descriptors().len());
    for (id, tool) in &defaults.tools {
        assert!(!tool.settings.is_empty(), "{id} saved nothing");
        assert!(tool.settings.values().all(|v| !v.is_null()), "{id}");
    }
    p.set_edit(&defaults).unwrap();
    assert_eq!(p.edit().unwrap(), defaults);
}
