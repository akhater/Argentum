//! The interface's side of the `ag` commands, generated from the Rust types:
//! `src/argentum/ag.generated.ts`, every argument and answer type, and
//! `AgCommands`, which `src/argentum/ag.ts` types each call by, and the event
//! names of `mods/events.rs` as `AG_EVENTS`. Ours.
//!
//! The command list is `commands!` in `mods/dispatch.rs`, so the file names
//! exactly the commands the app routes; the types are ts-rs's, as the
//! engine's contract (`engine/tests/contract.rs`). Test-only: nothing shipped
//! compiles ts-rs.
//!
//! This test fails when the file is not what the Rust types make now. To
//! write it, in src-tauri:
//!
//!     AG_WRITE_CONTRACT=1 cargo test --lib ag_contract

use std::any::TypeId;
use std::collections::{BTreeMap, HashSet};
use std::path::PathBuf;

use ts_rs::{TS, TypeVisitor};

/// What `dispatch::visit_commands` reports: each command, and every type its
/// arguments and answer are made of.
#[derive(Default)]
pub struct Commands {
    /// Name, then the TypeScript of its arguments and of its answer.
    commands: Vec<(&'static str, String, String)>,
    /// Each declared type's TypeScript, by its name.
    declared: BTreeMap<String, (TypeId, String)>,
    seen: HashSet<TypeId>,
    /// The events whose payload is a type of ours: event name, its TypeScript.
    event_payloads: Vec<(&'static str, String)>,
}

impl Commands {
    pub fn command<Args: TS + 'static, Answer: TS + 'static>(&mut self, name: &'static str) {
        self.commands.push((name, Args::name(), Answer::name()));
        self.reach::<Args>();
        self.reach::<Answer>();
    }

    pub fn command_without_args<Answer: TS + 'static>(&mut self, name: &'static str) {
        self.commands
            .push((name, "Record<string, never>".to_string(), Answer::name()));
        self.reach::<Answer>();
    }

    /// An event, by its name in `mods/events.rs`, whose payload is `Payload`.
    pub fn event<Payload: TS + 'static>(&mut self, name: &'static str) {
        self.event_payloads.push((name, Payload::name()));
        self.reach::<Payload>();
    }

    /// `T`, the types inside it, and theirs.
    fn reach<T: TS + 'static + ?Sized>(&mut self) {
        self.visit::<T>();
        T::visit_dependencies(self);
        T::visit_generics(self);
    }
}

impl TypeVisitor for Commands {
    fn visit<T: TS + 'static + ?Sized>(&mut self) {
        if !self.seen.insert(TypeId::of::<T>()) {
            return;
        }
        // A type of ours has a file of its own in ts-rs's terms; `String`,
        // `Option<T>` and the like are written inline and have none.
        if T::output_path().is_some() {
            let decl = format!("{}export {}\n", T::DOCS.unwrap_or(""), T::decl());
            if let Some((other, _)) = self.declared.get(&T::name())
                && *other != TypeId::of::<T>()
            {
                panic!(
                    "two Rust types are both `{}` in TypeScript: give one a \
                     `#[cfg_attr(test, ts(rename = \"...\"))]`",
                    T::name()
                );
            }
            self.declared.insert(T::name(), (TypeId::of::<T>(), decl));
        }
        T::visit_dependencies(self);
        T::visit_generics(self);
    }
}

/// Values under their places in the edit (`"/exposure": 0.7`), each as
/// untyped as the edit itself is on both sides: `Record<string, unknown>`.
/// For a field that holds them, through `#[ts(as = "...")]`.
pub struct EditValues;

impl TS for EditValues {
    type WithoutGenerics = Self;

    fn name() -> String {
        "Record<string, unknown>".to_string()
    }

    fn inline() -> String {
        Self::name()
    }

    fn inline_flattened() -> String {
        panic!("EditValues is not an object type of its own")
    }

    fn decl() -> String {
        panic!("EditValues is written inline")
    }

    fn decl_concrete() -> String {
        panic!("EditValues is written inline")
    }
}

fn generated_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../src/argentum/ag.generated.ts")
}

/// The file's text.
///
/// ts-rs writes a 64-bit integer as `bigint`; serde_json sends it as a JSON
/// number, and the interface reads it as one, so it is `number` here.
fn typescript() -> String {
    let mut commands = Commands::default();
    crate::mods::dispatch::visit_commands(&mut commands);
    // Kept in the edit, which no command types.
    commands.reach::<crate::mods::provider_look::ProviderLook>();
    // An event's, which no command answers.
    commands.reach::<crate::mods::lut_provider::LookProgress>();
    // Stored in the settings file and filled in by the export panel.
    commands.reach::<crate::app_settings::ExportPreset>();
    // The events whose payload is a type of ours.
    commands.event::<crate::mods::model_download::Progress>(crate::mods::events::AG_MODEL_DOWNLOAD);

    let mut out = String::from(
        "// Generated from the `commands!` table in src-tauri/src/mods/dispatch.rs\n\
         // by src-tauri/src/mods/ag_contract.rs. Do not edit: change the Rust types,\n\
         // then run `AG_WRITE_CONTRACT=1 cargo test --lib ag_contract` in src-tauri.\n\n",
    );
    for (_, decl) in commands.declared.values() {
        out += decl;
        out += "\n";
    }
    out += "/** Every `ag` command: what it takes and what it answers. */\n";
    out += "export interface AgCommands {\n";
    let mut sorted = commands.commands.clone();
    sorted.sort();
    for (name, args, answer) in sorted {
        out += &format!("  {name}: {{ args: {args}; answer: {answer} }};\n");
    }
    out += "}\n";
    out += "\n/** Every event the app sends, by its name in `mods/events.rs`. */\n";
    out += "export const AG_EVENTS = {\n";
    for (id, name) in crate::mods::events::ALL {
        out += &format!("  {id}: '{name}',\n");
    }
    out += "} as const;\n";
    out += "\n/** The payload of the events that have a type of ours, by event name. */\n";
    out += "export interface AgEventPayloads {\n";
    commands.event_payloads.sort();
    for (name, payload) in &commands.event_payloads {
        out += &format!("  '{name}': {payload};\n");
    }
    out += "}\n";
    replace_word(&out, "bigint", "number")
}

fn replace_word(text: &str, word: &str, with: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(i) = rest.find(word) {
        let before = rest[..i].chars().next_back();
        let after = rest[i + word.len()..].chars().next();
        let part_of_another = |c: Option<char>| c.is_some_and(|c| c.is_alphanumeric() || c == '_');
        out += &rest[..i];
        out += if part_of_another(before) || part_of_another(after) {
            word
        } else {
            with
        };
        rest = &rest[i + word.len()..];
    }
    out + rest
}

#[test]
fn the_generated_file_is_current() {
    let text = typescript();
    let path = generated_path();
    if std::env::var_os("AG_WRITE_CONTRACT").is_some() {
        std::fs::write(&path, &text).unwrap();
        eprintln!("wrote {}", path.display());
        return;
    }
    assert!(
        std::fs::read_to_string(&path).ok().as_deref() == Some(text.as_str()),
        "{} is stale: the Rust types now generate something else. Run \
         `AG_WRITE_CONTRACT=1 cargo test --lib ag_contract` in src-tauri and commit the result.",
        path.display()
    );
}

/// The keys ts-rs gives an object type: what each `name:` or `name?:` at the
/// top level of its declaration says.
fn declared_keys<T: TS>() -> Vec<String> {
    // Without the fields' doc comments, whose words are not keys.
    let mut decl = T::decl();
    while let Some(start) = decl.find("/**") {
        let end = decl[start..]
            .find("*/")
            .map_or(decl.len(), |i| start + i + 2);
        decl.replace_range(start..end, "");
    }
    let body = &decl[decl.find('{').expect("an object type") + 1..decl.rfind('}').unwrap()];
    let (mut keys, mut depth, mut word) = (Vec::new(), 0, String::new());
    for c in body.chars() {
        match c {
            '{' | '[' | '<' | '(' => depth += 1,
            '}' | ']' | '>' | ')' => depth -= 1,
            ':' if depth == 0 => keys.push(word.trim().trim_end_matches('?').to_string()),
            _ => {}
        }
        if depth == 0 && (c.is_alphanumeric() || c == '_') {
            word.push(c);
        } else if depth == 0 && !matches!(c, '?' | ' ') {
            word.clear();
        }
    }
    keys.sort();
    keys
}

/// A real answer, through serde, has exactly the keys its generated type
/// names: serde's renames, skips and flattening agree with ts-rs's.
#[test]
fn an_answer_has_the_keys_its_type_names() {
    let panels = crate::mods::editor_layout::LayoutPanels {
        navigator_open: true,
        tab: Some("history".to_string()),
        tab_width_px: 245,
        history_open: true,
        history_height_px: Some(180),
        snapshots_height_px: None,
        navigator_floating: false,
        navigator_at: Some([0.5, 0.5]),
        folded: vec!["crop.rotation".to_string()],
    };
    assert_eq!(
        keys_sent(&panels),
        declared_keys::<crate::mods::editor_layout::LayoutPanels>()
    );

    // A field renamed in a serde attribute ts-rs cannot read whole.
    let camera = crate::mods::profiles::Camera {
        make: "Canon".to_string(),
        model: "Canon EOS R6m3".to_string(),
        default_profile: Some("Canon EOS R6m3.dcp".to_string()),
    };
    assert_eq!(
        keys_sent(&camera),
        declared_keys::<crate::mods::profiles::Camera>()
    );
}

/// The export preset the panel fills in is this Rust type, not a second
/// hand-written one: every key it stores is a key the TypeScript names.
#[test]
fn an_export_preset_has_the_keys_its_type_names() {
    let presets = crate::app_settings::default_export_presets();
    let mut sent = serde_json::to_value(&presets[0]).unwrap();
    // serde sends a `None` as null, so the keys a preset may have are all there.
    assert!(sent.as_object().unwrap().contains_key("dpi"));
    sent["dpi"] = serde_json::json!(300);
    assert_eq!(
        keys_sent(&sent),
        declared_keys::<crate::app_settings::ExportPreset>()
    );
}

fn keys_sent(answer: &impl serde::Serialize) -> Vec<String> {
    let sent = serde_json::to_value(answer).unwrap();
    let mut keys: Vec<String> = sent.as_object().unwrap().keys().cloned().collect();
    keys.sort();
    keys
}

#[test]
fn bigint_is_replaced_only_as_a_word() {
    assert_eq!(
        replace_word("a: bigint; bigints: bigint_x", "bigint", "number"),
        "a: number; bigints: bigint_x"
    );
}
