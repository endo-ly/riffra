//! The fixed MCP tool surface and its action-to-Control-Command mapping.

use riffra_runtime::api::{
    batchable_command_names, command_params_allow_omitted, command_params_subschema,
};
use rmcp::model::{JsonObject, Tool, ToolAnnotations};
use schemars::SchemaGenerator;
use serde_json::{Map, Value, json};
use std::sync::Arc;

/// One MCP action and the Control Command it forwards to.
pub(crate) struct ActionDef {
    /// MCP action name; `None` for a tool that exposes one command directly.
    pub(crate) action: Option<&'static str>,
    pub(crate) command: &'static str,
}

/// One MCP tool.
pub(crate) struct ToolDef {
    pub(crate) name: &'static str,
    pub(crate) description: &'static str,
    pub(crate) actions: &'static [ActionDef],
    /// True only when every action leaves the Host state unchanged.
    pub(crate) read_only: bool,
    /// True when any action can remove or replace existing Project or library state.
    /// Additive-only and transient runtime tools remain false.
    pub(crate) destructive: bool,
}

impl ToolDef {
    pub(crate) fn action(&self, name: &str) -> Option<&'static ActionDef> {
        self.actions
            .iter()
            .find(|action| action.action == Some(name))
    }
}

/// Resolves the action selected by a tool call before schema validation.
pub(crate) fn resolve_action<'a>(
    tool: &'a ToolDef,
    arguments: &JsonObject,
) -> Result<&'a ActionDef, String> {
    match arguments.get("action") {
        Some(Value::String(name)) => tool
            .action(name)
            .ok_or_else(|| format!("unknown action for {}: {name}", tool.name)),
        Some(_) => Err(format!("action must be a string for {}", tool.name)),
        None => tool
            .actions
            .iter()
            .find(|action| action.action.is_none())
            .ok_or_else(|| format!("action is required for {}", tool.name)),
    }
}

const fn action(action: &'static str, command: &'static str) -> ActionDef {
    ActionDef {
        action: Some(action),
        command,
    }
}

const fn direct(command: &'static str) -> ActionDef {
    ActionDef {
        action: None,
        command,
    }
}

/// Every MCP tool in stable presentation order.
pub(crate) const TOOLS: &[ToolDef] = &[
    ToolDef {
        name: "host_info",
        description: "Inspect the running Riffra Host. Use status for connection identity, info for version and capabilities, and bootstrap for the initial project snapshot.",
        actions: &[
            action("status", "host.status"),
            action("info", "host.info"),
            action("bootstrap", "host.bootstrap"),
        ],
        read_only: true,
        destructive: false,
    },
    ToolDef {
        name: "host_shutdown",
        description: "Request graceful shutdown of the running Riffra Host.",
        actions: &[direct("host.shutdown")],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "project",
        description: "List, create, open, rename, import, or export Riffra Projects. Open switches the Host's active Project.",
        actions: &[
            action("list", "project.list"),
            action("create", "project.create"),
            action("open", "project.open"),
            action("rename", "project.rename"),
            action("import", "project.import"),
            action("import_score", "project.import-score"),
            action("export", "project.export"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "project_restore",
        description: "Restore one saved generation of the active Project.",
        actions: &[direct("project.restore-generation")],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "session_read",
        description: "Read session state. Use inspect for a lightweight agent view with optional musical range and Track scope; use get only when the complete canonical session is required.",
        actions: &[
            action("get", "session.get"),
            action("inspect", "session.inspect"),
        ],
        read_only: true,
        destructive: false,
    },
    ToolDef {
        name: "session_edit",
        description: "Edit session-wide settings or apply several canonical changes atomically. Batch operations accept trackName and clipName references and bind names for later operations.",
        actions: &[
            action("apply", "session.apply"),
            action("settings_update", "session.settings.update"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "history",
        description: "Read the Host's mutation history, undo the latest change, or redo it.",
        actions: &[
            action("get", "history.get"),
            action("undo", "history.undo"),
            action("redo", "history.redo"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "track",
        description: "List, add, update, remove, duplicate, or reorder timeline Tracks and their mix controls.",
        actions: &[
            action("list", "track.list"),
            action("add", "track.add"),
            action("update", "track.update"),
            action("remove", "track.remove"),
            action("duplicate", "track.duplicate"),
            action("reorder", "track.reorder"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "track_input",
        description: "Set or clear audio, MIDI, and external audio-input routes on a Track.",
        actions: &[
            action("audio_input_set", "track.audio-input.set"),
            action("audio_input_clear", "track.audio-input.clear"),
            action("midi_input_set", "track.midi-input.set"),
            action("midi_input_clear", "track.midi-input.clear"),
            action("external_audio_input_set", "track.external-audio-input.set"),
            action(
                "external_audio_input_clear",
                "track.external-audio-input.clear",
            ),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "clip_audio",
        description: "List and edit Audio Clips. Move accepts several Clips in one command; use update for a single Clip.",
        actions: &[
            action("list", "clip.audio.list"),
            action("add_asset", "clip.audio.add-asset"),
            action("update", "clip.audio.update"),
            action("move", "clip.audio.move"),
            action("trim", "clip.audio.trim"),
            action("split", "clip.audio.split"),
            action("duplicate", "clip.audio.duplicate"),
            action("crossfade", "clip.audio.crossfade"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "clip_midi",
        description: "List and edit MIDI Clips in timeline ticks. Move accepts several Clips in one command; use update for a single Clip.",
        actions: &[
            action("list", "clip.midi.list"),
            action("create", "clip.midi.create"),
            action("add_asset", "clip.midi.add-asset"),
            action("update", "clip.midi.update"),
            action("move", "clip.midi.move"),
            action("trim", "clip.midi.trim"),
            action("split", "clip.midi.split"),
            action("merge", "clip.midi.merge"),
            action("duplicate", "clip.midi.duplicate"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "clip_midi_note",
        description: "Edit MIDI Notes in Clip-relative ticks using numeric MIDI pitches. Use update_many for one Note as well as several.",
        actions: &[
            action("insert", "clip.midi.note.insert"),
            action("update_many", "clip.midi.note.update-many"),
            action("remove_many", "clip.midi.note.remove-many"),
            action("quantize", "clip.midi.note.quantize"),
            action("transform", "clip.midi.note.transform"),
            action("duplicate", "clip.midi.note.duplicate"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "clip_midi_event",
        description: "List and edit precise instrument-control events such as sustain, pitch bend, aftertouch, and parameter changes.",
        actions: &[
            action("list", "clip.midi.instrument-event.list"),
            action("add", "clip.midi.instrument-event.add"),
            action("set", "clip.midi.instrument-event.set"),
            action("update", "clip.midi.instrument-event.update"),
            action("remove", "clip.midi.instrument-event.remove"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "clip_edit",
        description: "Remove several Clips together or paste Clips at a timeline position.",
        actions: &[
            action("remove", "clip.remove"),
            action("paste", "clip.paste"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "music_note",
        description: "Work with Notes in musical coordinates: bar:beat positions, note names, and whole-note durations.",
        actions: &[
            action("list", "music.note.list"),
            action("get", "music.note.get"),
            action("insert", "music.note.insert"),
            action("update", "music.note.update"),
            action("remove", "music.note.remove"),
            action("transform", "music.note.transform"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "music_midi",
        description: "Create or resize MIDI Clips in musical coordinates, or export the selected Clip or Track as a Standard MIDI File.",
        actions: &[
            action("clip_create", "music.midi-clip.create"),
            action("clip_resize", "music.midi-clip.resize"),
            action("export_midi", "music.export-midi"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "music_timebase",
        description: "Read or replace the tempo and time-signature map.",
        actions: &[
            action("get_map", "music.timebase.get-map"),
            action("update", "music.timebase.update"),
            action("set_map", "music.timebase.set-map"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "music_structure",
        description: "Add, update, remove, and list timeline Markers and Regions.",
        actions: &[
            action("marker_add", "music.marker.add"),
            action("marker_update", "music.marker.update"),
            action("marker_remove", "music.marker.remove"),
            action("region_list", "music.region.list"),
            action("region_add", "music.region.add"),
            action("region_update", "music.region.update"),
            action("region_remove", "music.region.remove"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "music_harmony",
        description: "Resolve chord symbols or list, insert, update, remove, and realize harmony events.",
        actions: &[
            action("resolve", "music.harmony.resolve"),
            action("list", "music.harmony.list"),
            action("insert", "music.harmony.insert"),
            action("update", "music.harmony.update"),
            action("remove", "music.harmony.remove"),
            action("realize", "music.harmony.realize"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "music_phrase",
        description: "Preview or insert a rhythm or phrase pattern into a MIDI Clip.",
        actions: &[
            action("preview", "music.phrase.preview"),
            action("insert", "music.phrase.insert"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "mixer",
        description: "Inspect the mixer, route a Track output, preview channel controls, or set and preview Master gain.",
        actions: &[
            action("get", "mixer.get"),
            action("output_set", "mixer.output.set"),
            action("channel_preview", "mixer.channel.preview"),
            action("master_gain_set", "audio.master-gain.set"),
            action("master_gain_preview", "audio.master-gain.preview"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "mixer_bus",
        description: "List, add, update, or remove Group and Return mixing channels.",
        actions: &[
            action("list", "mixer.bus.list"),
            action("add", "mixer.bus.add"),
            action("update", "mixer.bus.update"),
            action("remove", "mixer.bus.remove"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "mixer_send",
        description: "List, add, update, remove, or preview parallel Track-to-Return Sends.",
        actions: &[
            action("list", "mixer.send.list"),
            action("add", "mixer.send.add"),
            action("update", "mixer.send.update"),
            action("remove", "mixer.send.remove"),
            action("preview", "mixer.send.preview"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "mixer_sidechain",
        description: "List, add, update, remove, or preview sidechain routes to a device.",
        actions: &[
            action("list", "mixer.sidechain.list"),
            action("add", "mixer.sidechain.add"),
            action("update", "mixer.sidechain.update"),
            action("remove", "mixer.sidechain.remove"),
            action("preview", "mixer.sidechain.preview"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "automation",
        description: "List, read, replace, clear, or preview timeline automation lanes.",
        actions: &[
            action("list", "automation.list"),
            action("get", "automation.get"),
            action("set", "automation.set"),
            action("clear", "automation.clear"),
            action("preview", "automation.preview"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "instrument",
        description: "Assign Instruments and edit Instrument MIDI settings or VST3 instrument slots.",
        actions: &[
            action("save", "instrument.save"),
            action("export", "instrument.export"),
            action("apply", "instrument.apply"),
            action("midi_get", "instrument.midi.get"),
            action("midi_set", "instrument.midi.set"),
            action("vst3_set", "instrument.vst3.set"),
            action("clear", "instrument.clear"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "instrument_library",
        description: "List the Instrument Library and update favorites, categories, and tags.",
        actions: &[
            action("list", "library.instrument.list"),
            action("favorite_set", "library.instrument.favorite.set"),
            action("category_list", "library.instrument.category.list"),
            action("category_set", "library.instrument.category.set"),
            action("tags_set", "library.instrument.tags.set"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "instrument_collection",
        description: "List, create, rename, delete, and update memberships of Instrument collections.",
        actions: &[
            action("list", "library.instrument.collection.list"),
            action("create", "library.instrument.collection.create"),
            action("rename", "library.instrument.collection.rename"),
            action("delete", "library.instrument.collection.delete"),
            action(
                "membership_set",
                "library.instrument.collection.membership.set",
            ),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "preview_audio",
        description: "Start or stop live preview of an Asset or Instrument.",
        actions: &[
            action("asset_start", "asset.preview"),
            action("asset_stop", "asset.stop-preview"),
            action("instrument_start", "instrument.preview"),
            action("instrument_stop", "instrument.preview.stop"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "effect",
        description: "Add, remove, or reorder effects on a Track, Bus, or Master chain.",
        actions: &[
            action("add", "effect.add"),
            action("remove", "effect.remove"),
            action("reorder", "effect.reorder"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "device",
        description: "Inspect a device, bypass it, or list, read, format, and set plugin parameters.",
        actions: &[
            action("inspect", "device.inspect"),
            action("bypass", "device.bypass"),
            action("parameter_list", "device.parameter.list"),
            action("parameter_get", "device.parameter.get"),
            action("parameter_format", "device.parameter.format"),
            action("parameter_set", "device.parameter.set"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "plugin_catalog",
        description: "List installed plugins, scan synchronously, or start a background scan.",
        actions: &[
            action("list", "plugin.catalog.list"),
            action("scan", "plugin.scan"),
            action("scan_start", "plugin.scan.start"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "plugin_preset",
        description: "List plugin presets, read one preset, or select a preset.",
        actions: &[
            action("list", "plugin.preset.list"),
            action("get", "plugin.preset.get"),
            action("set", "plugin.preset.set"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "plugin_state",
        description: "Read, replace, or import a saved VST3 device state. For JUCE plugins, inspect shows the decoded structure, read returns one value, and patch edits ValueTree properties or embedded XML in place, so a large state never has to pass through the call.",
        actions: &[
            action("get", "plugin.state.get"),
            action("set", "plugin.state.set"),
            action("import", "plugin.state.import"),
            action("inspect", "plugin.state.inspect"),
            action("read", "plugin.state.read"),
            action("patch", "plugin.state.patch"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "plugin_ui",
        description: "Open a plugin editor on a Project device or audition a plugin outside the Project.",
        actions: &[
            action("editor_open", "plugin.editor.open"),
            action("audition_open", "plugin.audition.open"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "asset",
        description: "Import Standard MIDI Files, search the Library, update Asset tags, or list related Assets.",
        actions: &[
            action("import_midi", "asset.import-midi"),
            action("search", "asset.search"),
            action("update", "asset.update"),
            action("related", "asset.related"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "missing",
        description: "List missing Assets and plugins, relink or replace them, or disable a missing plugin.",
        actions: &[
            action("list", "missing.list"),
            action("relink", "missing.relink"),
            action("disable_plugin", "missing.disable-plugin"),
            action("replace_plugin", "missing.replace-plugin"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "record",
        description: "Control recording and manage recorded takes, including listing, renaming, archiving, promoting, tagging, deleting, and duplicate detection.",
        actions: &[
            action("punch_set", "record.punch.set"),
            action("start", "record.start"),
            action("stop", "record.stop"),
            action("status", "record.status"),
            action("list", "record.list"),
            action("rename", "record.rename"),
            action("archive", "record.archive"),
            action("promote", "record.promote"),
            action("tag", "record.tag"),
            action("delete", "record.delete"),
            action("duplicates", "record.duplicates"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "take",
        description: "Activate a recorded take, place a separate Clip, select an audio take variant, or run take comparison.",
        actions: &[
            action("activate", "take.activate"),
            action("place_separate_clip", "take.place-separate-clip"),
            action("variant_set", "clip.audio.take-variant.set"),
            action("comparison_start", "take.comparison.start"),
            action("comparison_switch", "take.comparison.switch"),
            action("comparison_stop", "take.comparison.stop"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "render",
        description: "Start a Render or stem job, export or list completed renders, or delete one managed Render.",
        actions: &[
            action("start", "render.start"),
            action("stems", "render.stems"),
            action("export", "render.export"),
            action("list", "render.list"),
            action("delete", "render.delete"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "render_settings",
        description: "Read or update the active Project's render settings.",
        actions: &[
            action("get", "render.settings.get"),
            action("set", "render.settings.set"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "job",
        description: "Read or cancel a background job. Wait polls job.get until a terminal state or a timeout without cancelling the job.",
        actions: &[
            action("get", "job.get"),
            action("cancel", "job.cancel"),
            action("wait", "job.get"),
        ],
        read_only: false,
        destructive: true,
    },
    ToolDef {
        name: "analysis",
        description: "Inspect an Asset or WAV file, or start a background loudness measurement job.",
        actions: &[
            action("inspect", "analysis.inspect"),
            action("measure", "analysis.measure"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "transport",
        description: "Play, pause, stop, return to the start, seek, or set the transport loop range.",
        actions: &[
            action("play", "transport.play"),
            action("pause", "transport.pause"),
            action("stop", "transport.stop"),
            action("go_to_start", "transport.go-to-start"),
            action("seek", "transport.seek"),
            action("loop_set", "transport.loop.set"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "midi",
        description: "Enable or disable live MIDI listening, send raw MIDI, target live input, or panic all notes.",
        actions: &[
            action("listening_enable", "midi.listening.enable"),
            action("listening_disable", "midi.listening.disable"),
            action("send", "midi.send"),
            action("target_set", "midi.target.set"),
            action("panic", "midi.panic"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "audio",
        description: "Inspect audio status and diagnostics, probe devices, recover audio, and inspect or retry runtime projection.",
        actions: &[
            action("status", "audio.status"),
            action("diagnostics", "audio.diagnostics"),
            action("probe", "audio.probe"),
            action("probe_channels", "audio.probe-channels"),
            action("recover", "audio.recover"),
            action("startup_retry", "audio.startup-retry"),
            action("projection_get", "audio.projection.get"),
            action("projection_retry", "audio.projection.retry"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "audio_driver",
        description: "Read or change the native audio driver configuration.",
        actions: &[
            action("get", "audio.driver.get"),
            action("set", "audio.driver.set"),
        ],
        read_only: false,
        destructive: false,
    },
    ToolDef {
        name: "audio_safety",
        description: "Engage emergency mute or reset feedback protection.",
        actions: &[
            action("emergency_mute", "audio.emergency-mute"),
            action(
                "feedback_protection_reset",
                "audio.feedback-protection.reset",
            ),
        ],
        read_only: false,
        destructive: false,
    },
];

/// A callable tool with its compiled input validators.
pub(crate) struct ToolSpec {
    pub(crate) definition: &'static ToolDef,
    pub(crate) tool: Tool,
    #[cfg(test)]
    pub(crate) validator: jsonschema::Validator,
    action_validators: Vec<ActionValidator>,
}

struct ActionValidator {
    action: Option<&'static str>,
    validator: jsonschema::Validator,
}

impl ToolSpec {
    pub(crate) fn action_validator(&self, action: Option<&str>) -> Option<&jsonschema::Validator> {
        self.action_validators
            .iter()
            .find(|validator| validator.action == action)
            .map(|validator| &validator.validator)
    }
}

struct ToolSchema {
    root: Value,
    actions: Vec<Value>,
}

/// Builds the fixed tool list and compiles each input schema once.
pub(crate) fn build_tools() -> Result<Vec<ToolSpec>, String> {
    TOOLS
        .iter()
        .map(|definition| {
            let schema = tool_input_schema(definition)?;
            #[cfg(test)]
            let validator = jsonschema::Validator::new(&schema.root)
                .map_err(|error| format!("{} input schema is invalid: {error}", definition.name))?;
            let action_validators = definition
                .actions
                .iter()
                .zip(&schema.actions)
                .map(|(action, schema)| {
                    let validator = jsonschema::Validator::new(schema).map_err(|error| {
                        format!(
                            "{}.{} input schema is invalid: {error}",
                            definition.name,
                            action.action.unwrap_or("direct")
                        )
                    })?;
                    Ok(ActionValidator {
                        action: action.action,
                        validator,
                    })
                })
                .collect::<Result<Vec<_>, String>>()?;
            let input_schema: JsonObject = schema
                .root
                .as_object()
                .cloned()
                .ok_or_else(|| format!("{} input schema is not an object", definition.name))?;
            let mut tool = Tool::default();
            tool.name = definition.name.into();
            tool.description = Some(definition.description.into());
            tool.input_schema = Arc::new(input_schema);
            let mut annotations = ToolAnnotations::new();
            if definition.read_only {
                annotations.read_only_hint = Some(true);
            }
            annotations.destructive_hint = Some(definition.destructive);
            tool.annotations = Some(annotations);
            Ok(ToolSpec {
                definition,
                tool,
                #[cfg(test)]
                validator,
                action_validators,
            })
        })
        .collect()
}

fn tool_input_schema(definition: &'static ToolDef) -> Result<ToolSchema, String> {
    let mut generator = SchemaGenerator::default();
    let mut actions = Vec::new();
    for action in definition.actions {
        actions.push(action_branch(definition, action, &mut generator)?);
    }
    let mut definitions = generator.take_definitions(false);
    if definition.name == "session_edit" {
        constrain_batch_commands(&mut definitions)?;
    }
    let mut action_schemas = Vec::with_capacity(actions.len());
    for action in &actions {
        let mut action = action.clone();
        if !definitions.is_empty() {
            let Value::Object(object) = &mut action else {
                return Err(format!(
                    "{} action schema is not an object",
                    definition.name
                ));
            };
            object.insert("$defs".into(), Value::Object(definitions.clone()));
        }
        action_schemas.push(action);
    }
    let mut root = Map::new();
    root.insert("type".into(), json!("object"));
    if definition.actions.len() == 1 && definition.actions[0].action.is_none() {
        let branch = actions[0].clone();
        let Value::Object(branch) = branch else {
            return Err(format!(
                "{} action schema is not an object",
                definition.name
            ));
        };
        root.extend(branch);
    } else {
        root.insert("oneOf".into(), Value::Array(actions));
    }
    if !definitions.is_empty() {
        root.insert("$defs".into(), Value::Object(definitions));
    }
    Ok(ToolSchema {
        root: Value::Object(root),
        actions: action_schemas,
    })
}

fn constrain_batch_commands(definitions: &mut Map<String, Value>) -> Result<(), String> {
    let operation_name = {
        let session_apply = definitions
            .get("SessionApplyParams")
            .ok_or_else(|| "session.apply schema is missing SessionApplyParams".to_owned())?;
        session_apply
            .get("properties")
            .and_then(Value::as_object)
            .and_then(|properties| properties.get("operations"))
            .and_then(|operations| operations.get("items"))
            .and_then(|items| items.get("$ref"))
            .and_then(Value::as_str)
            .and_then(|reference| reference.strip_prefix("#/$defs/"))
            .ok_or_else(|| {
                "session.apply operations schema is missing its item reference".to_owned()
            })?
            .to_owned()
    };
    let command = definitions
        .get_mut(&operation_name)
        .and_then(|operation| operation.get_mut("properties"))
        .and_then(Value::as_object_mut)
        .and_then(|properties| properties.get_mut("command"))
        .and_then(Value::as_object_mut)
        .ok_or_else(|| format!("{operation_name} schema is missing its command property"))?;
    command.insert(
        "enum".into(),
        Value::Array(
            batchable_command_names()
                .map(|name| Value::String(name.to_owned()))
                .collect(),
        ),
    );
    Ok(())
}

fn action_branch(
    tool: &'static ToolDef,
    action: &'static ActionDef,
    generator: &mut SchemaGenerator,
) -> Result<Value, String> {
    let mut properties = Map::new();
    if let Some(action_name) = action.action {
        properties.insert("action".into(), json!({ "const": action_name }));
        properties.insert(
            "params".into(),
            if tool.name == "job" && action_name == "wait" {
                json!({
                    "type": "object",
                    "description": "Job reference and optional wait budget.",
                    "properties": {
                        "id": { "type": "string", "description": "Background job ID." },
                        "timeoutMs": {
                            "type": "integer",
                            "minimum": 0,
                            "description": "Maximum time to wait in milliseconds; omit to wait indefinitely."
                        }
                    },
                    "required": ["id"],
                    "additionalProperties": false
                })
            } else {
                command_params_subschema(action.command, generator)
                    .ok_or_else(|| {
                        format!(
                            "{}.{} references unknown command {}",
                            tool.name,
                            action_name,
                            action.command
                        )
                    })?
                    .to_value()
            },
        );
    } else {
        let allows_omitted = command_params_allow_omitted(action.command).ok_or_else(|| {
            format!(
                "{} references unknown command {}",
                tool.name, action.command
            )
        })?;
        let params = command_params_subschema(action.command, generator)
            .ok_or_else(|| {
                format!(
                    "{} references unknown command {}",
                    tool.name, action.command
                )
            })?
            .to_value();
        if !allows_omitted {
            properties.insert("params".into(), params);
        }
    }
    properties.insert(
        "expectedProjectId".into(),
        json!({
            "type": "string",
            "description": "Require the command to run against this Project."
        }),
    );
    properties.insert(
        "expectedSequence".into(),
        json!({
            "type": "integer",
            "minimum": 0,
            "description": "Require the Project to be at this canonical sequence."
        }),
    );
    let mut required = Vec::new();
    if action.action.is_some() {
        required.push(json!("action"));
    }
    if properties.contains_key("params") && !params_optional(action) {
        required.push(json!("params"));
    }
    let mut branch = Map::new();
    branch.insert("type".into(), json!("object"));
    branch.insert("properties".into(), Value::Object(properties));
    if !required.is_empty() {
        branch.insert("required".into(), Value::Array(required));
    }
    branch.insert("additionalProperties".into(), json!(false));
    Ok(Value::Object(branch))
}

fn params_optional(action: &ActionDef) -> bool {
    command_params_allow_omitted(action.command).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;
    use riffra_runtime::api::{command_names, command_params_allow_omitted};

    #[test]
    fn the_tool_surface_matches_the_fixed_design() {
        // Arrange
        let expected = [
            "host_info",
            "host_shutdown",
            "project",
            "project_restore",
            "session_read",
            "session_edit",
            "history",
            "track",
            "track_input",
            "clip_audio",
            "clip_midi",
            "clip_midi_note",
            "clip_midi_event",
            "clip_edit",
            "music_note",
            "music_midi",
            "music_timebase",
            "music_structure",
            "music_harmony",
            "music_phrase",
            "mixer",
            "mixer_bus",
            "mixer_send",
            "mixer_sidechain",
            "automation",
            "instrument",
            "instrument_library",
            "instrument_collection",
            "preview_audio",
            "effect",
            "device",
            "plugin_catalog",
            "plugin_preset",
            "plugin_state",
            "plugin_ui",
            "asset",
            "missing",
            "record",
            "take",
            "render",
            "render_settings",
            "job",
            "analysis",
            "transport",
            "midi",
            "audio",
            "audio_driver",
            "audio_safety",
        ];

        // Act
        let names: Vec<_> = TOOLS.iter().map(|tool| tool.name).collect();

        // Assert
        assert_eq!(names, expected);
        let actions: Vec<_> = TOOLS.iter().flat_map(|tool| tool.actions.iter()).collect();
        assert_eq!(actions.len(), 216);
        let commands: std::collections::HashSet<_> =
            actions.iter().map(|action| action.command).collect();
        assert_eq!(commands.len(), 215);
        assert_eq!(command_names().len(), 219);
        let public_commands: std::collections::HashSet<_> =
            command_names().iter().copied().collect();
        let mut unused = public_commands
            .difference(&commands)
            .copied()
            .collect::<Vec<_>>();
        unused.sort_unstable();
        assert_eq!(
            unused,
            [
                "clip.midi.note.add",
                "clip.midi.note.clear",
                "clip.midi.note.update",
                "instrument.list",
            ]
        );
        for tool in TOOLS {
            for action in tool.actions {
                assert!(
                    command_params_allow_omitted(action.command).is_some(),
                    "{} references {}",
                    tool.name,
                    action.command
                );
            }
        }
        let job = TOOLS.iter().find(|tool| tool.name == "job").unwrap();
        assert_eq!(
            job.actions
                .iter()
                .map(|action| action.action)
                .collect::<Vec<_>>(),
            [Some("get"), Some("cancel"), Some("wait")]
        );
    }

    #[test]
    fn destructive_hints_mark_tools_that_remove_or_replace_state() {
        // Arrange
        let expected = [
            "host_shutdown",
            "project_restore",
            "session_edit",
            "history",
            "track",
            "track_input",
            "clip_audio",
            "clip_midi",
            "clip_midi_note",
            "clip_midi_event",
            "clip_edit",
            "music_note",
            "music_timebase",
            "music_structure",
            "music_harmony",
            "mixer_bus",
            "mixer_send",
            "mixer_sidechain",
            "automation",
            "instrument",
            "instrument_collection",
            "effect",
            "plugin_catalog",
            "plugin_preset",
            "plugin_state",
            "missing",
            "record",
            "render",
            "job",
        ];

        // Act
        let destructive: Vec<_> = TOOLS
            .iter()
            .filter(|tool| tool.destructive)
            .map(|tool| tool.name)
            .collect();

        // Assert
        assert_eq!(destructive, expected);
        for tool in TOOLS {
            assert!(
                !(tool.read_only && tool.destructive),
                "{} cannot be both read-only and destructive",
                tool.name
            );
        }
    }

    #[test]
    fn every_tool_schema_reference_resolves_inside_its_own_definitions() {
        // Arrange
        let tools = build_tools().unwrap();

        // Act / Assert
        for tool in tools {
            let schema = Value::Object((*tool.tool.input_schema).clone());
            let mut references = Vec::new();
            collect_references(&schema, &mut references);
            for reference in references {
                let pointer = reference.strip_prefix('#').unwrap_or(&reference);
                assert!(
                    schema.pointer(pointer).is_some(),
                    "{} has an unresolved reference {reference}",
                    tool.tool.name
                );
            }
        }
    }

    #[test]
    fn every_action_schema_reference_resolves_inside_its_own_definitions() {
        // Arrange / Act / Assert
        for definition in TOOLS {
            let schema = tool_input_schema(definition).unwrap();
            for (index, action) in schema.actions.iter().enumerate() {
                let mut references = Vec::new();
                collect_references(action, &mut references);
                for reference in references {
                    let pointer = reference.strip_prefix('#').unwrap_or(&reference);
                    assert!(
                        action.pointer(pointer).is_some(),
                        "{} action {index} has an unresolved reference {reference}",
                        definition.name
                    );
                }
            }
        }
    }

    fn collect_references(value: &Value, references: &mut Vec<String>) {
        match value {
            Value::Object(object) => {
                if let Some(reference) = object.get("$ref").and_then(Value::as_str) {
                    references.push(reference.to_owned());
                }
                for value in object.values() {
                    collect_references(value, references);
                }
            }
            Value::Array(values) => {
                for value in values {
                    collect_references(value, references);
                }
            }
            _ => {}
        }
    }

    #[test]
    fn action_validator_reports_the_nested_field_path() {
        // Arrange
        let tools = build_tools().unwrap();
        let tool = tools
            .iter()
            .find(|tool| tool.tool.name == "clip_audio")
            .unwrap();
        let action = tool.definition.action("update").unwrap();
        let validator = tool.action_validator(action.action).unwrap();

        // Act
        let input = json!({
            "action": "update",
            "params": {"clipId": "clip:1", "patch": {"muted": "yes"}}
        });
        let error = validator.iter_errors(&input).next().unwrap();

        // Assert
        assert_eq!(error.instance_path().to_string(), "/params/patch/muted");
    }

    #[test]
    fn session_apply_schema_lists_batchable_commands() {
        // Arrange
        let definition = TOOLS
            .iter()
            .find(|tool| tool.name == "session_edit")
            .unwrap();
        let schema = tool_input_schema(definition).unwrap().root;
        let commands = schema
            .pointer("/$defs/BatchOperation/properties/command/enum")
            .and_then(Value::as_array)
            .unwrap()
            .iter()
            .map(|value| value.as_str().unwrap())
            .collect::<Vec<_>>();
        let expected = batchable_command_names().collect::<Vec<_>>();

        // Act / Assert
        assert_eq!(commands, expected);
        assert!(commands.contains(&"track.add"));
        assert!(commands.contains(&"clip.midi.create"));
        assert!(commands.contains(&"music.note.insert"));
        assert!(!commands.contains(&"session.apply"));
        assert!(!commands.contains(&"track.list"));
        assert!(!commands.contains(&"render.start"));
    }

    #[test]
    fn session_apply_validator_rejects_non_batchable_commands() {
        // Arrange
        let tools = build_tools().unwrap();
        let tool = tools
            .iter()
            .find(|tool| tool.tool.name == "session_edit")
            .unwrap();
        let validator = tool.action_validator(Some("apply")).unwrap();
        let input = |command: &str| {
            json!({
                "action": "apply",
                "params": {"operations": [{"command": command, "params": {}}]}
            })
        };

        // Act / Assert
        assert!(validator.is_valid(&input("track.add")));
        assert!(!validator.is_valid(&input("track.list")));
        assert!(!validator.is_valid(&input("session.apply")));
    }

    #[test]
    fn schemas_enforce_action_and_params_structure() {
        // Arrange
        let tools = build_tools().unwrap();
        let tool = |name: &str| tools.iter().find(|tool| tool.tool.name == name).unwrap();

        // Act / Assert
        assert!(tool("clip_audio").validator.is_valid(&json!({
            "action": "list"
        })));
        assert!(tool("clip_audio").validator.is_valid(&json!({
            "action": "update",
            "params": {"clipId": "clip:1", "patch": {"muted": true}}
        })));
        assert!(!tool("clip_audio").validator.is_valid(&json!({
            "action": "update"
        })));
        assert!(!tool("clip_audio").validator.is_valid(&json!({
            "action": "unknown"
        })));
        assert!(!tool("clip_audio").validator.is_valid(&json!({
            "action": "list",
            "unexpected": true
        })));
        assert!(tool("host_shutdown").validator.is_valid(&json!({})));
        assert!(!tool("host_shutdown").validator.is_valid(&json!({
            "params": {}
        })));
        assert!(tool("project_restore").validator.is_valid(&json!({
            "params": {"fileName": "generation.riffra"}
        })));
        assert!(!tool("project_restore").validator.is_valid(&json!({})));
        assert!(tool("job").validator.is_valid(&json!({
            "action": "wait",
            "params": {"id": "job:1", "timeoutMs": 1000}
        })));
        assert!(!tool("job").validator.is_valid(&json!({
            "action": "wait",
            "params": {"id": "job:1", "timeoutMs": -1}
        })));
    }
}
