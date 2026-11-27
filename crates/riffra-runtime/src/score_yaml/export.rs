//! Canonical production state to a Score song folder.
//!
//! A Project is read from its canonical state, rendered as a Score document,
//! and written as a song folder together with the plugin states, instrument
//! packages, and audio files the document refers to. The folder is staged in a
//! temporary directory and renamed into place only after every file exists.

mod performance;
mod production;

use crate::api::output::ScoreExportOmission;
use crate::instrument::BuiltInInstrumentCatalog;
use crate::score::{self, emit};
use crate::score_yaml::linked::LinkedScore;
use num_rational::BigRational;
use riffra_core::{CreativeSession, MidiClip, PanLaw, Track, TrackInstrument, TrackKind};
use serde_json::{Map, Value, json};
use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};

#[derive(Debug, thiserror::Error)]
#[error("{message}")]
pub(crate) struct ExportError {
    pub(crate) message: String,
    pub(crate) details: Value,
}

impl ExportError {
    pub(super) fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
            details: Value::Null,
        }
    }
}

impl From<String> for ExportError {
    fn from(message: String) -> Self {
        Self::new(message)
    }
}

/// One file of the song folder, either generated or copied from its source.
#[derive(Debug, PartialEq, Eq)]
pub(super) enum ExportFile {
    /// Bytes Riffra generated for this file.
    Bytes(Vec<u8>),
    /// An existing file copied into the staged folder.
    Copy(PathBuf),
}

/// One Score section occurrence resolved from the arrangement.
#[derive(Clone, Debug)]
pub(super) struct SongSection {
    /// Region name the section came from, before disambiguation.
    pub(super) base: String,
    /// Final Score section name.
    pub(super) name: String,
    pub(super) start_tick: u64,
    pub(super) end_tick: u64,
    /// Exact length of every bar, in ticks.
    pub(super) bar_lengths: Vec<u64>,
    pub(super) bar_starts: Vec<u64>,
    pub(super) meter: (u8, u8),
    pub(super) bpm: f64,
    pub(super) tonic: String,
    pub(super) scale: Option<String>,
    /// Whether `tonic` comes from the linked Score instead of the default.
    pub(super) inherited: bool,
}

impl SongSection {
    pub(super) fn bars(&self) -> usize {
        self.bar_lengths.len()
    }

    /// Length of the final bar when it is shorter than a complete bar.
    pub(super) fn last_bar_quarters(&self, ppq: u32) -> Option<String> {
        let last = *self.bar_lengths.last()?;
        (last < self.full_bar_ticks(ppq)).then(|| quarters(last, ppq))
    }

    fn full_bar_ticks(&self, ppq: u32) -> u64 {
        let (numerator, denominator) = self.meter;
        u64::from(numerator) * 4 * u64::from(ppq) / u64::from(denominator)
    }

    /// Converts an absolute tick to a Score position inside this section.
    pub(super) fn position(&self, tick: u64, ppq: u32) -> Value {
        position_in(&self.bar_starts, tick, ppq)
    }
}

fn position_in(bar_starts: &[u64], tick: u64, ppq: u32) -> Value {
    let index = bar_starts.partition_point(|start| *start <= tick).max(1) - 1;
    let bar = index + 1;
    let offset = tick.saturating_sub(bar_starts[index]);
    let steps = i128::from(offset) * 4;
    if steps % i128::from(ppq) == 0 {
        json!({"bar": bar, "step": u64::try_from(steps / i128::from(ppq)).unwrap_or(0) + 1})
    } else {
        json!({"bar": bar, "offset_quarters": rational(offset, ppq)})
    }
}

/// Formats a tick count as a Score quarter-note quantity.
pub(super) fn rational(ticks: u64, ppq: u32) -> String {
    let value = BigRational::new(ticks.into(), ppq.into());
    fraction(&value)
}

pub(super) fn quarters(ticks: u64, ppq: u32) -> String {
    rational(ticks, ppq)
}

pub(super) fn fraction(value: &BigRational) -> String {
    if value.is_integer() {
        value.to_integer().to_string()
    } else {
        format!("{}/{}", value.numer(), value.denom())
    }
}

pub(super) fn to_quarters(ticks: u64, ppq: u32) -> BigRational {
    BigRational::new(ticks.into(), ppq.into())
}

/// Converts a quarter-note position back to ticks, when exact.
pub(super) fn ticks(value: &BigRational, ppq: u32) -> Option<u64> {
    let scaled = value * BigRational::from_integer(ppq.into());
    scaled.is_integer().then(|| {
        use num_traits::ToPrimitive;
        scaled.to_integer().to_u64()
    })?
}

/// A Track that becomes a Score instrument, kit, or audio instrument.
#[derive(Clone, Debug)]
pub(super) struct ExportTrack<'a> {
    pub(super) id: String,
    pub(super) track: &'a Track,
    pub(super) role: TrackRole,
}

impl ExportTrack<'_> {
    /// The Score instrument ID the canonical Track came from, without a kit prefix.
    pub(super) fn source_id(&self) -> Option<&str> {
        self.track
            .score_instrument
            .as_deref()
            .map(|id| id.strip_prefix("kit:").unwrap_or(id))
    }
}

#[derive(Clone, Debug)]
pub(super) enum TrackRole {
    Instrument,
    Kit(String),
    Audio,
}

/// One kit member resolved from the kit's notes.
#[derive(Clone, Debug)]
pub(super) struct KitMember {
    pub(super) id: String,
    pub(super) trigger: u8,
    /// Score ID the member carried over from the source declaration.
    pub(super) source: Option<String>,
}

pub(super) struct Export<'a> {
    pub(super) session: &'a CreativeSession,
    pub(super) data_root: &'a Path,
    pub(super) catalog: &'a BuiltInInstrumentCatalog,
    pub(super) scanner: &'a Path,
    pub(super) ppq: u32,
    pub(super) sections: Vec<SongSection>,
    pub(super) tracks: Vec<ExportTrack<'a>>,
    pub(super) kits: BTreeMap<String, Vec<KitMember>>,
    pub(super) original: Option<LinkedScore>,
    pub(super) omissions: Vec<ScoreExportOmission>,
    /// Files of the song folder by their relative path, generated or copied at write time.
    pub(super) files: BTreeMap<String, ExportFile>,
    /// WAV files written for each shared audio asset.
    pub(super) audio_files: BTreeMap<riffra_core::AssetId, String>,
    /// Cached plugin parameter names by device id.
    pub(super) parameters: BTreeMap<String, Vec<PluginParameter>>,
    /// Articulation declarations the exported events use, by Score instrument.
    pub(super) written_articulations: BTreeMap<String, BTreeSet<String>>,
}

/// One automatable plugin parameter reported by the scanner.
pub(super) struct PluginParameter {
    pub(super) id: Option<String>,
    pub(super) name: String,
    pub(super) index: u32,
}

/// A planned song folder and its file list.
pub(crate) struct ExportPlan {
    pub(crate) path: PathBuf,
    pub(crate) omissions: Vec<ScoreExportOmission>,
    /// Score instrument ID written for each canonical Track.
    score_instruments: BTreeMap<String, String>,
    files: BTreeMap<String, ExportFile>,
    score: String,
}

impl ExportPlan {
    /// Writes the staged folder at the requested destination.
    ///
    /// # Errors
    /// Returns an error when the destination appears or a file cannot be written.
    pub(crate) fn write(&self) -> Result<PathBuf, ExportError> {
        if self.path.exists() {
            return Err(ExportError::new(format!(
                "the destination already exists: {}",
                self.path.display()
            )));
        }
        stage(&self.path, &self.files, &self.score)?;
        Ok(self.path.join("score.yaml"))
    }

    /// Returns the Session with the Score instrument IDs this plan wrote.
    pub(crate) fn linked_session(&self, session: &CreativeSession) -> CreativeSession {
        let mut linked = session.clone();
        for track in &mut linked.arrangement.tracks {
            if let Some(id) = self.score_instruments.get(&track.id) {
                track.score_instrument = Some(id.clone());
            }
        }
        linked
    }
}

/// Renders a Session as a Score song folder without writing it.
///
/// # Errors
/// Returns an error when the destination exists or a source cannot be
/// expressed concretely, such as a plugin missing from the catalog.
pub(crate) fn plan(
    session: &CreativeSession,
    path: &Path,
    data_root: &Path,
    catalog: &BuiltInInstrumentCatalog,
    scanner: &Path,
) -> Result<ExportPlan, ExportError> {
    if path.exists() {
        return Err(ExportError::new(format!(
            "the destination already exists: {}",
            path.display()
        )));
    }
    let original = LinkedScore::load(session);
    let clock = Clock::new(&session.arrangement.timebase);
    let song_end = song_end(session, &clock);
    let mut omissions = Vec::new();
    let sections = clock.sections(session, song_end, original.as_ref(), &mut omissions);
    let (tracks, mut used) = resolve_tracks(session, &mut omissions);
    let mut export = Export {
        session,
        data_root,
        catalog,
        scanner,
        ppq: clock.ppq,
        sections,
        tracks,
        kits: BTreeMap::new(),
        original,
        omissions,
        files: BTreeMap::new(),
        audio_files: BTreeMap::new(),
        parameters: BTreeMap::new(),
        written_articulations: BTreeMap::new(),
    };
    export.kits = kit_members(&export, &mut used);
    let mut document = Map::new();
    performance::apply(&mut export, &mut document)?;
    production::apply(&mut export, &mut document)?;
    let score = emit::yaml(&Value::Object(document));
    if let Err(error) = score::compile(&score) {
        return Err(ExportError {
            message: format!("the exported Score is not valid: {}", error.message),
            details: json!({"path": error.path}),
        });
    }
    Ok(ExportPlan {
        path: path.to_path_buf(),
        omissions: export.omissions,
        score_instruments: export
            .tracks
            .iter()
            .map(|track| {
                let id = match &track.role {
                    TrackRole::Kit(kit) => format!("kit:{kit}"),
                    _ => track.id.clone(),
                };
                (track.track.id.clone(), id)
            })
            .collect(),
        files: export.files,
        score,
    })
}

/// Reports one Session object the Score cannot express.
pub(super) fn omit(
    omissions: &mut Vec<ScoreExportOmission>,
    subject: impl Into<String>,
    message: impl Into<String>,
) {
    omissions.push(ScoreExportOmission {
        subject: subject.into(),
        message: message.into(),
    });
}

/// Assigns unique Score identifiers from names, in insertion order.
pub(super) fn unique_id(base: &str, fallback: &str, used: &mut BTreeSet<String>) -> String {
    let stem = identifier(base).unwrap_or_else(|| fallback.to_owned());
    let mut candidate = stem.clone();
    let mut number = 2;
    while !used.insert(candidate.clone()) {
        candidate = format!("{stem}_{number}");
        number += 1;
    }
    candidate
}

/// Normalizes text into a Score identifier, or `None` when nothing remains.
pub(super) fn identifier(text: &str) -> Option<String> {
    let mut result = String::new();
    for character in text.chars() {
        if character.is_ascii_alphanumeric() {
            result.push(character.to_ascii_lowercase());
        } else if character == '_' || !result.ends_with('_') {
            result.push('_');
        }
    }
    let result = result.trim_matches('_').to_owned();
    (!result.is_empty()).then_some(result)
}

/// Produces a stable identifier suffix for a Score file name.
pub(super) fn file_identifier(text: &str) -> String {
    identifier(text).unwrap_or_else(|| "item".into())
}

impl LinkedScore {
    /// Returns the section display note declared for a Score section.
    pub(super) fn section_notes(&self, section: &str) -> Option<&str> {
        self.field(&["sections", section, "notes"])
            .and_then(Value::as_str)
    }

    /// Returns the raw instrument declaration by Score ID.
    pub(super) fn instrument(&self, id: &str) -> Option<&Value> {
        self.field(&["instruments", id])
    }

    /// Returns the build-up declarations of one Score instrument or kit.
    pub(super) fn articulations(&self, id: &str) -> Option<&Value> {
        self.instrument(id)
            .and_then(|spec| spec.get("source"))
            .and_then(|source| source.get("articulations"))
            .or_else(|| self.field(&["kits", id, "source", "articulations"]))
    }

    /// Returns the transitions block verbatim.
    pub(super) fn transitions(&self) -> Option<&Value> {
        self.field(&["transitions"])
    }
}

/// The musical clock used by the arrangement.
struct Clock {
    ppq: u32,
    tempos: Vec<(u64, f64)>,
    meters: Vec<(u64, u8, u8)>,
}

impl Clock {
    fn new(timebase: &riffra_core::ProjectTimebase) -> Self {
        Self {
            ppq: timebase.ppq.max(1),
            tempos: timebase
                .tempo_changes
                .iter()
                .map(|change| (change.tick, change.bpm))
                .collect(),
            meters: timebase
                .time_signature_changes
                .iter()
                .map(|change| (change.tick, change.numerator, change.denominator))
                .collect(),
        }
    }

    fn bpm_at(&self, tick: u64) -> f64 {
        self.tempos
            .iter()
            .take_while(|(at, _)| *at <= tick)
            .last()
            .map_or_else(
                || self.tempos.first().map_or(120.0, |(_, bpm)| *bpm),
                |(_, bpm)| *bpm,
            )
    }

    fn meter_at(&self, tick: u64) -> (u8, u8) {
        self.meters
            .iter()
            .take_while(|(at, ..)| *at <= tick)
            .last()
            .map_or_else(
                || self.meters.first().map_or((4, 4), |(_, n, d)| (*n, *d)),
                |(_, n, d)| (*n, *d),
            )
    }

    fn bar_ticks(&self, start: u64) -> u64 {
        let (numerator, denominator) = self.meter_at(start);
        u64::from(numerator) * 4 * u64::from(self.ppq) / u64::from(denominator)
    }

    fn bar_end(&self, tick: u64) -> u64 {
        let start = self.bar_start(tick);
        start + self.bar_ticks(start)
    }

    fn bar_start(&self, tick: u64) -> u64 {
        let mut cursor = 0;
        loop {
            let end = cursor + self.bar_ticks(cursor);
            if tick < end {
                return cursor;
            }
            if !self
                .meters
                .iter()
                .any(|(at, ..)| *at > cursor && *at <= end)
            {
                let bars = (tick - end) / self.bar_ticks(end);
                cursor = end + bars * self.bar_ticks(end);
                continue;
            }
            cursor = end;
        }
    }

    fn is_bar_start(&self, tick: u64) -> bool {
        self.bar_start(tick) == tick
    }
}

/// Returns the tick where the Score's musical content ends.
fn content_end(session: &CreativeSession) -> u64 {
    let mut end = 0;
    for clip in &session.arrangement.midi_clips {
        if clip.muted {
            continue;
        }
        for note in &clip.notes {
            end = end.max(clip.start_tick.0 + note.start_tick.0 + note.duration_ticks);
        }
        for event in &clip.events {
            let tick = clip.start_tick.0.saturating_add(event.tick.0);
            end = end.max(tick.saturating_add(1));
        }
        for event in &clip.instrument_control_events {
            let tick = clip.start_tick.0.saturating_add(event.tick.0);
            let length = match event.kind {
                riffra_core::InstrumentControlEventKind::ParameterRamp {
                    duration_ticks, ..
                } if duration_ticks > 0 => duration_ticks,
                _ => 1,
            };
            end = end.max(tick.saturating_add(length));
        }
    }
    for clip in &session.arrangement.audio_clips {
        if clip.muted {
            continue;
        }
        end = end.max(
            clip.start_tick.0
                + session
                    .arrangement
                    .timebase
                    .duration_to_ticks(clip.start_tick, clip.timeline_duration),
        );
    }
    for lane in &session.arrangement.automation_lanes {
        if let Some(point) = lane.points.last() {
            end = end.max(point.tick.0);
        }
    }
    for region in &session.arrangement.regions {
        end = end.max(region.end_tick.0);
    }
    for event in &session.arrangement.harmony_events {
        end = end.max(event.end_tick.0);
    }
    end
}

fn song_end(session: &CreativeSession, clock: &Clock) -> u64 {
    let content = content_end(session);
    let configured = session.settings.mixdown.musical_end_tick;
    if configured != 0 {
        return configured.max(content);
    }
    if content == 0 {
        clock.bar_ticks(0).max(1)
    } else {
        clock.bar_end(content.saturating_sub(1))
    }
}

impl Clock {
    /// Resolves the arrangement's regions into Score sections.
    fn sections(
        &self,
        session: &CreativeSession,
        end: u64,
        original: Option<&LinkedScore>,
        omissions: &mut Vec<ScoreExportOmission>,
    ) -> Vec<SongSection> {
        let regions = &session.arrangement.regions;
        let mut spans: Vec<(String, u64, u64)> = Vec::new();
        let usable = regions.iter().all(|region| {
            self.is_bar_start(region.start_tick.0)
                && (region.end_tick.0 == end || self.is_bar_start(region.end_tick.0))
                && region.start_tick.0 < region.end_tick.0
                && region.end_tick.0 <= end
        }) && regions.windows(2).all(|pair| {
            pair[0].end_tick.0 <= pair[1].start_tick.0
                && pair[0].start_tick.0 <= pair[1].start_tick.0
        });
        if regions.is_empty() {
            spans.push(("song".into(), 0, end));
        } else if usable {
            let mut cursor = 0;
            let mut gap = 1;
            for region in regions {
                if region.start_tick.0 > cursor {
                    spans.push((format!("untitled_{gap}"), cursor, region.start_tick.0));
                    gap += 1;
                }
                spans.push((region.name.clone(), region.start_tick.0, region.end_tick.0));
                cursor = region.end_tick.0;
            }
            if cursor < end {
                spans.push((format!("untitled_{gap}"), cursor, end));
            }
        } else {
            omit(
                omissions,
                "regions",
                "regions overlap, nest, or are not separated at bar boundaries",
            );
            spans.push(("song".into(), 0, end));
        }
        let mut sections = Vec::new();
        for (base, span_start, span_end) in spans {
            for (part, start, stop) in self.split_meters(span_start, span_end, omissions) {
                let name = if part == 0 {
                    base.clone()
                } else {
                    format!("{base}_{}", self.bar_number(span_start, start))
                };
                sections.push(self.section(&base, &name, start, stop, original));
            }
        }
        sections
    }

    /// Counts the bar containing `tick`, from the start of its span.
    fn bar_number(&self, span_start: u64, tick: u64) -> usize {
        let mut cursor = span_start;
        let mut bars = 1;
        while cursor < tick {
            let end = self.bar_end(cursor);
            if end > tick {
                break;
            }
            cursor = end;
            bars += 1;
        }
        bars
    }

    /// Splits a span at the bar lines where its meter changes.
    fn split_meters(
        &self,
        start: u64,
        end: u64,
        omissions: &mut Vec<ScoreExportOmission>,
    ) -> Vec<(usize, u64, u64)> {
        let mut bounds = vec![start];
        for (at, numerator, denominator) in &self.meters {
            if *at <= start || *at >= end {
                continue;
            }
            if self.is_bar_start(*at) {
                bounds.push(*at);
                continue;
            }
            let split = self.bar_end(*at);
            omit(
                omissions,
                format!("meter change at tick {at}"),
                format!(
                    "{numerator}/{denominator} starts inside a bar, so it takes effect at tick {split}"
                ),
            );
            if split < end {
                bounds.push(split);
            }
        }
        bounds.sort_unstable();
        bounds.dedup();
        bounds
            .iter()
            .enumerate()
            .map(|(part, from)| {
                let to = bounds.get(part + 1).copied().unwrap_or(end);
                (part, *from, to)
            })
            .collect()
    }

    fn section(
        &self,
        base: &str,
        name: &str,
        start: u64,
        end: u64,
        original: Option<&LinkedScore>,
    ) -> SongSection {
        let mut bar_starts = Vec::new();
        let mut bar_lengths = Vec::new();
        let mut cursor = start;
        while cursor < end {
            let mut length = self.bar_ticks(cursor);
            if cursor + length > end {
                length = end - cursor;
            }
            bar_starts.push(cursor);
            bar_lengths.push(length);
            cursor += length;
            if !self.is_bar_start(cursor) {
                break;
            }
        }
        if bar_starts.is_empty() {
            bar_starts.push(start);
            bar_lengths.push(self.bar_ticks(start));
        }
        let mut tonics = defaults();
        if let Some(original) = original
            && let Some(occurrence) = original.occurrence(base, start, self.ppq)
        {
            tonics = (occurrence.tonic.clone(), occurrence.scale.clone(), true);
        }
        SongSection {
            base: base.to_owned(),
            name: name.to_owned(),
            start_tick: start,
            end_tick: end,
            bar_lengths,
            bar_starts,
            meter: self.meter_at(start),
            bpm: self.bpm_at(start),
            tonic: tonics.0,
            scale: tonics.1,
            inherited: tonics.2,
        }
    }
}

fn defaults() -> (String, Option<String>, bool) {
    ("C".into(), None, false)
}

/// Resolves every Track that becomes a Score instrument or kit.
fn resolve_tracks<'a>(
    session: &'a CreativeSession,
    omissions: &mut Vec<ScoreExportOmission>,
) -> (Vec<ExportTrack<'a>>, BTreeSet<String>) {
    let mut used = BTreeSet::new();
    let mut tracks = Vec::new();
    for (index, track) in session.arrangement.tracks.iter().enumerate() {
        let base = track
            .score_instrument
            .as_deref()
            .and_then(identifier)
            .or_else(|| identifier(&track.name))
            .unwrap_or_else(|| format!("track_{}", index + 1));
        let role = match track.kind {
            TrackKind::Audio => TrackRole::Audio,
            TrackKind::Instrument if track.instrument.is_none() => {
                if has_performance(session, &track.id) {
                    omissions.push(ScoreExportOmission {
                        subject: format!("track \"{}\"", track.name),
                        message: "the track has performance content but no instrument".into(),
                    });
                }
                continue;
            }
            TrackKind::Instrument => match track.score_instrument.as_deref() {
                Some(score) if score.starts_with("kit:") => TrackRole::Kit(
                    identifier(&score[4..]).unwrap_or_else(|| format!("kit_{}", index + 1)),
                ),
                _ => TrackRole::Instrument,
            },
        };
        let base = match &role {
            TrackRole::Kit(kit) => kit.clone(),
            _ => base,
        };
        let id = unique_id(&base, &format!("track_{}", index + 1), &mut used);
        let role = match role {
            TrackRole::Kit(_) => TrackRole::Kit(id.clone()),
            role => role,
        };
        tracks.push(ExportTrack { id, track, role });
    }
    (tracks, used)
}

fn has_performance(session: &CreativeSession, track_id: &str) -> bool {
    session.arrangement.midi_clips.iter().any(|clip| {
        clip.track_id == track_id
            && !clip.muted
            && (!clip.notes.is_empty()
                || !clip.events.is_empty()
                || !clip.instrument_control_events.is_empty())
    })
}

/// Resolves kit member identifiers from the kit's trigger notes.
fn kit_members(export: &Export, used: &mut BTreeSet<String>) -> BTreeMap<String, Vec<KitMember>> {
    let mut kits = BTreeMap::new();
    for track in &export.tracks {
        let TrackRole::Kit(kit) = &track.role else {
            continue;
        };
        let mut triggers = BTreeSet::new();
        for clip in clips_for(export.session, &track.track.id) {
            if clip.muted {
                continue;
            }
            for note in &clip.notes {
                triggers.insert(note.note);
            }
        }
        let source_kit = track.source_id();
        let mut parts = Vec::new();
        for trigger in triggers {
            let source = source_kit.and_then(|kit| {
                export
                    .original
                    .as_ref()
                    .and_then(|original| original_kit_part(original, kit, trigger))
            });
            let base = source.clone().unwrap_or_else(|| format!("{kit}_{trigger}"));
            let id = unique_id(&base, &format!("{kit}_{trigger}"), used);
            parts.push(KitMember {
                id,
                trigger,
                source,
            });
        }
        kits.insert(kit.clone(), parts);
    }
    kits
}

fn original_kit_part(original: &LinkedScore, kit: &str, trigger: u8) -> Option<String> {
    let instruments = original.field(&["instruments"])?.as_object()?;
    instruments.iter().find_map(|(id, spec)| {
        (spec.get("kit").and_then(Value::as_str) == Some(kit)
            && spec
                .get("trigger_note_midi")
                .and_then(Value::as_u64)
                .and_then(|note| u8::try_from(note).ok())
                == Some(trigger))
        .then(|| id.clone())
    })
}

/// Converts a fixed pan under one pan law to its linear-balance equivalent.
pub(super) fn pan_law(law: PanLaw, gain_db: f64, pan: f64) -> (f64, f64) {
    if law == PanLaw::LinearBalance {
        return (gain_db, pan);
    }
    let angle = (pan + 1.0) * std::f64::consts::PI / 4.0;
    let (left, right) = if law == PanLaw::EqualPower {
        (angle.cos(), angle.sin())
    } else {
        (
            (std::f64::consts::SQRT_2 * angle.cos()).min(1.0),
            (std::f64::consts::SQRT_2 * angle.sin()).min(1.0),
        )
    };
    if left >= right {
        (gain_db + 20.0 * left.log10(), right / left - 1.0)
    } else {
        (gain_db + 20.0 * right.log10(), 1.0 - left / right)
    }
}

/// Returns the unmuted MIDI clips of one Track.
pub(super) fn clips_for<'a>(
    session: &'a CreativeSession,
    track_id: &'a str,
) -> impl Iterator<Item = &'a MidiClip> {
    session
        .arrangement
        .midi_clips
        .iter()
        .filter(move |clip| clip.track_id == track_id)
}

/// Writes the staged song folder and renames it into place.
fn stage(
    path: &Path,
    files: &BTreeMap<String, ExportFile>,
    score: &str,
) -> Result<(), ExportError> {
    let parent = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."));
    std::fs::create_dir_all(parent).map_err(|error| {
        ExportError::new(format!("could not create {}: {error}", parent.display()))
    })?;
    let staging = parent.join(format!(
        ".riffra-export-{}",
        riffra_control::new_instance_id()
    ));
    std::fs::create_dir(&staging).map_err(|error| {
        ExportError::new(format!("could not create {}: {error}", staging.display()))
    })?;
    let result = (|| {
        for (relative, file) in files {
            let target = staging.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    ExportError::new(format!("could not create {}: {error}", parent.display()))
                })?;
            }
            match file {
                ExportFile::Bytes(bytes) => std::fs::write(&target, bytes).map_err(|error| {
                    ExportError::new(format!("could not write {}: {error}", target.display()))
                })?,
                ExportFile::Copy(source) => {
                    std::fs::copy(source, &target).map_err(|error| {
                        ExportError::new(format!(
                            "could not copy {} to {}: {error}",
                            source.display(),
                            target.display()
                        ))
                    })?;
                }
            }
        }
        std::fs::write(staging.join("score.yaml"), score)
            .map_err(|error| ExportError::new(format!("could not write score.yaml: {error}")))?;
        std::fs::rename(&staging, path).map_err(|error| {
            ExportError::new(format!("could not create {}: {error}", path.display()))
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// Registers a package directory's files for the staged song folder.
pub(super) fn copy_package(
    export: &mut Export,
    source: &Path,
    destination: &str,
) -> Result<(), ExportError> {
    copy_tree(source, destination, export)
}

fn copy_tree(directory: &Path, destination: &str, export: &mut Export) -> Result<(), ExportError> {
    let mut entries = std::fs::read_dir(directory)
        .map_err(|error| {
            ExportError::new(format!("could not read {}: {error}", directory.display()))
        })?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|error| {
            ExportError::new(format!("could not read {}: {error}", directory.display()))
        })?;
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for entry in entries {
        let name = entry.file_name().to_string_lossy().into_owned();
        if name.starts_with('.') {
            continue;
        }
        let path = entry.path();
        let relative = format!("{destination}/{name}");
        if entry
            .file_type()
            .map_err(|error| ExportError::new(error.to_string()))?
            .is_dir()
        {
            copy_tree(&path, &relative, export)?;
        } else {
            add_file(export, relative, ExportFile::Copy(path))?;
        }
    }
    Ok(())
}

/// Adds one relative file to the staged folder.
pub(super) fn add_file(
    export: &mut Export,
    relative: String,
    file: ExportFile,
) -> Result<(), ExportError> {
    if export.files.insert(relative.clone(), file).is_some() {
        return Err(ExportError::new(format!(
            "two Score files share the path {relative}"
        )));
    }
    Ok(())
}

/// Returns the Score ID of the Track that owns a canonical Track.
pub(super) fn track_score_id(export: &Export, track_id: &str) -> Option<String> {
    export
        .tracks
        .iter()
        .find(|track| track.track.id == track_id)
        .map(|track| track.id.clone())
}

/// Returns the Score instrument ID of a kit used by a canonical Track.
pub(super) fn kit_of(export: &Export, track_id: &str) -> Option<String> {
    export
        .tracks
        .iter()
        .find(|track| track.track.id == track_id)
        .and_then(|track| match &track.role {
            TrackRole::Kit(kit) => Some(kit.clone()),
            _ => None,
        })
}

/// Returns the instrument assignment of a Track, if it hosts one.
pub(super) fn instrument_of(track: &Track) -> Option<&TrackInstrument> {
    track.instrument.as_ref()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::api::output::{PluginEntry, PluginFormat, PluginRole, PluginScanState, ScanReport};
    use riffra_core::{
        CreativeSession, InstrumentControlEventKind, MidiClip, MidiNote, MixBusKind, TimelineTick,
    };

    const SCORE: &str = r#"{
        "meta":{"title":"Export","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[
            {"section":"verse","bars":2,"tonic":"C","scale":"ionian"},
            {"section":"chorus","bars":2,"tonic":"A","scale":"aeolian","bpm":132}
        ],
        "sections":{
            "verse":{
                "lines":{"main":{"instrument":"lead","melody":"verse_line"},"bass":{"instrument":"bass","melody":"bass_line","register":"bass"}},
                "chords":["1:maj","6:m7"],
                "events":[
                    {"kind":"cc","instrument":"lead","at":{"bar":1,"step":3},"number":11,"value":96}
                ]
            },
            "chorus":{
                "drums":"beat",
                "lines":{"main":{"instrument":"lead","melody":"chorus_line"}}
            }
        },
        "melodies":{
            "verse_line":["1@64:8 3:8","5:8 1':8"],
            "bass_line":["1:16","5:16"],
            "chorus_line":["6:8 b7:8","1':16"]
        },
        "drums":{"beat":{"parts":{"kick":["x... .... .... ...."],"snare":[".... .... x... ...."]}}},
        "instruments":{
            "lead":{"source":{"vst3":{"plugin":"Synth"}},"gain_db":-3},
            "bass":{"source":{"vst3":{"plugin":"Bass"}}},
            "kick":{"kit":"drums","trigger_note_midi":36},
            "snare":{"kit":"drums","trigger_note_midi":38}
        },
        "kits":{"drums":{"source":{"vst3":{"plugin":"Drums"}}}},
        "mix":{
            "buses":{"rhythm":{"kind":"group"},"room":{"kind":"return","gain_db":-6}},
            "tracks":{"lead":{"output":"rhythm","sends":{"room":-12}},"drums":{"output":"rhythm"}},
            "automation":[{"target":{"instrument":"lead","parameter":"volume"},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":0},{"at":{"bar":2,"step":9},"value":-6}]}]
        }
    }"#;

    struct TempRoot(PathBuf);

    impl std::ops::Deref for TempRoot {
        type Target = Path;

        fn deref(&self) -> &Self::Target {
            &self.0
        }
    }

    impl AsRef<Path> for TempRoot {
        fn as_ref(&self) -> &Path {
            &self.0
        }
    }

    impl Drop for TempRoot {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn root(name: &str) -> TempRoot {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-export-{name}-{}",
            riffra_control::new_instance_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        TempRoot(root)
    }

    fn catalog(root: &Path) {
        let entries = [
            ("Synth", PluginRole::Instrument),
            ("Bass", PluginRole::Instrument),
            ("Drums", PluginRole::Instrument),
            ("EQ", PluginRole::Effect),
        ]
        .into_iter()
        .map(|(name, role)| {
            let path = root.join(format!("{name}.vst3"));
            std::fs::create_dir_all(&path).unwrap();
            PluginEntry {
                id: name.into(),
                name: name.into(),
                vendor: None,
                version: None,
                identifier: Some(format!("{name}-class")),
                format: PluginFormat::Vst3,
                role: Some(role),
                path: path.to_string_lossy().into_owned(),
                bundle: true,
                modified_at_ms: None,
                scan_state: PluginScanState::Validated,
                realtime_warm_up_seconds: None,
            }
        })
        .collect();
        crate::plugins::catalog::save(
            root,
            &ScanReport {
                root: root.to_string_lossy().into_owned(),
                started_at_ms: 0,
                finished_at_ms: 0,
                plugins: entries,
                issues: Vec::new(),
            },
        )
        .unwrap();
    }

    fn exported(
        session: &CreativeSession,
        root: &Path,
        catalog: &BuiltInInstrumentCatalog,
    ) -> ExportPlan {
        let plan = plan(
            session,
            &root.join("song"),
            root,
            catalog,
            Path::new("unused"),
        )
        .expect("export");
        plan.write().expect("write");
        plan
    }

    fn midi_clip(id: &str, track_id: &str) -> MidiClip {
        MidiClip {
            id: id.into(),
            name: id.into(),
            track_id: track_id.into(),
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: 3840,
            notes: Vec::new(),
            events: Vec::new(),
            instrument_control_events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        }
    }

    fn built_in_catalog(root: &Path, definition: &str) -> BuiltInInstrumentCatalog {
        let resources = root.join("built-ins");
        std::fs::create_dir_all(resources.join("preset")).unwrap();
        std::fs::write(resources.join("preset").join("definition.json"), definition).unwrap();
        let manifest = json!({
            "sourceRelease": "test",
            "presets": [{
                "id": "TEST-001", "name": "Test", "category": "Test", "tags": ["test"],
                "recommendedRange": {"minMidi": 0, "maxMidi": 127},
                "preview": {"tempoBpm": 120.0, "ticksPerBeat": 480,
                    "timeSignature": {"numerator": 4, "denominator": 4},
                    "lengthTicks": 1920,
                    "notes": [{"tick": 0, "durationTicks": 480, "note": 60, "velocity": 100}]},
                "definitionPath": "preset/definition.json", "resourceBasePath": "preset"
            }]
        });
        std::fs::write(resources.join("manifest.json"), manifest.to_string()).unwrap();
        BuiltInInstrumentCatalog::load(&resources).unwrap()
    }

    fn add_played_track(
        session: &mut CreativeSession,
        name: &str,
        instrument: TrackInstrument,
        notes: &[u8],
    ) -> String {
        let mut track = session.arrangement.tracks[0].clone();
        track.id = format!("track:{name}");
        track.name = name.into();
        track.kind = TrackKind::Instrument;
        track.score_instrument = None;
        track.instrument = Some(instrument);
        track.effects.clear();
        track.output_bus_id = None;
        track.external_audio_source_track_ids.clear();
        let id = track.id.clone();
        session.arrangement.tracks.push(track);
        let mut clip = midi_clip(&format!("clip:{name}"), &id);
        clip.notes = notes
            .iter()
            .enumerate()
            .map(|(index, note)| MidiNote {
                id: format!("note:{name}:{index}"),
                lyric: None,
                note: *note,
                start_tick: TimelineTick(index as u64 * 480),
                duration_ticks: 480,
                velocity: 100,
                channel: 1,
            })
            .collect();
        session.arrangement.midi_clips.push(clip);
        id
    }

    #[test]
    fn identifiers_are_stable_and_fall_back_to_track_positions() {
        assert_eq!(identifier("Lead Vocal"), Some("lead_vocal".into()));
        assert_eq!(identifier("Keys / Pad"), Some("keys_pad".into()));
        assert_eq!(identifier("日本語"), None);

        let mut used = BTreeSet::new();
        assert_eq!(unique_id("lead", "track_1", &mut used), "lead");
        assert_eq!(unique_id("lead", "track_2", &mut used), "lead_2");
        assert_eq!(unique_id("lead", "track_3", &mut used), "lead_3");
    }

    #[test]
    fn audio_end_follows_the_tempo_map_and_sample_rate() {
        use riffra_core::{
            AudioTakeVariant, FadeShape, FrameDuration, FrameRange, TempoChange, TimelineTick,
        };
        let mut session = CreativeSession::new(1);
        session.arrangement.timebase.tempo_changes = vec![TempoChange {
            tick: 0,
            bpm: 240.0,
        }];
        let frames = FrameDuration {
            frames: 96_000,
            sample_rate: 48_000,
        };
        session
            .arrangement
            .audio_clips
            .push(riffra_core::AudioClip {
                id: "clip".into(),
                track_id: "track".into(),
                asset_id: riffra_core::mint_asset_id(),
                start_tick: TimelineTick(0),
                source_range: FrameRange {
                    start: 0,
                    end: 96_000,
                },
                source_sample_rate: 48_000,
                timeline_duration: frames,
                gain_db: 0.0,
                pan: 0.0,
                fade_in: FrameDuration {
                    frames: 0,
                    sample_rate: 48_000,
                },
                fade_out: FrameDuration {
                    frames: 0,
                    sample_rate: 48_000,
                },
                fade_shape: FadeShape::EqualPower,
                loop_enabled: false,
                muted: false,
                name: "clip".into(),
                recording_take_id: None,
                take_variant: AudioTakeVariant::Raw,
            });
        let clock = Clock::new(&session.arrangement.timebase);
        assert_eq!(session.arrangement.timebase.ppq, 960);
        assert_eq!(song_end(&session, &clock), 7680);
    }

    #[test]
    fn pan_law_conversion_keeps_both_effective_gains() {
        let linear = |gain_db: f64, pan: f64| {
            let gain = 10.0_f64.powf(gain_db / 20.0);
            (
                gain * if pan > 0.0 { 1.0 - pan } else { 1.0 },
                gain * if pan <= 0.0 { 1.0 + pan } else { 1.0 },
            )
        };
        for law in [PanLaw::EqualPower, PanLaw::UnityCenterStereo] {
            for gain_db in [0.0, -6.0, 3.0] {
                for pan in [-1.0, -0.3, 0.0, 0.4, 1.0] {
                    let angle = (pan + 1.0) * std::f64::consts::PI / 4.0;
                    let scale = if law == PanLaw::UnityCenterStereo {
                        std::f64::consts::SQRT_2
                    } else {
                        1.0
                    };
                    let amplitude = 10.0_f64.powf(gain_db / 20.0);
                    let expected = (
                        amplitude * (angle.cos() * scale).min(1.0),
                        amplitude * (angle.sin() * scale).min(1.0),
                    );
                    let (converted_gain, out_pan) = pan_law(law, gain_db, pan);
                    let converted = linear(converted_gain, out_pan);
                    assert!(
                        (converted.0 - expected.0).abs() < 1e-9,
                        "{law:?} {gain_db} {pan}"
                    );
                    assert!(
                        (converted.1 - expected.1).abs() < 1e-9,
                        "{law:?} {gain_db} {pan}"
                    );
                }
            }
        }
        assert_eq!(pan_law(PanLaw::LinearBalance, -6.0, 0.5), (-6.0, 0.5));
    }

    #[test]
    fn quarter_quantities_are_exact_fractions() {
        assert_eq!(rational(0, 480), "0");
        assert_eq!(rational(480, 480), "1");
        assert_eq!(rational(160, 480), "1/3");
        assert_eq!(fraction(&BigRational::new(3.into(), 2.into())), "3/2");
    }

    #[test]
    fn exported_song_folder_is_readable_and_repeatable() {
        let root = root("roundtrip");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let session = crate::score_yaml::import::session_from_score(
            SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let first = exported(&session, &root, catalog);
        assert!(
            first.omissions.iter().all(|omission| omission
                .message
                .contains("evaluates this volume automation linearly")),
            "{:?}",
            first.omissions
        );
        let text = std::fs::read_to_string(first.path.join("score.yaml")).unwrap();
        crate::score::compile(&text).expect("the exported Score compiles");

        let imported = crate::score_yaml::import::session_from_score(
            &text,
            &root.join("song"),
            &root,
            catalog,
        );
        assert_eq!(
            performance(&session),
            performance(&imported),
            "the round trip keeps the performance"
        );
        let second = plan(
            &imported,
            &root.join("second"),
            &root,
            catalog,
            Path::new("unused"),
        )
        .expect("second export");
        second.write().expect("write");
        let text = std::fs::read_to_string(root.join("second/score.yaml")).unwrap();
        assert_eq!(
            text,
            std::fs::read_to_string(first.path.join("score.yaml")).unwrap()
        );
        assert_eq!(first.files, second.files, "bundled files are deterministic");
    }

    /// The musical content the round trip must preserve.
    #[derive(Debug, PartialEq)]
    struct PerformanceView {
        tracks: BTreeMap<String, TrackView>,
        tempo: Vec<(u64, f64)>,
        meters: Vec<(u64, u8, u8)>,
        buses: Vec<(String, MixBusKind)>,
    }

    #[derive(Debug, PartialEq)]
    struct TrackView {
        notes: Vec<(u64, u64, u8, u8)>,
        controls: Vec<(u64, InstrumentControlEventKind)>,
    }

    fn performance(session: &CreativeSession) -> PerformanceView {
        let mut tracks = BTreeMap::new();
        for track in &session.arrangement.tracks {
            let Some(id) = &track.score_instrument else {
                continue;
            };
            let mut notes = Vec::new();
            let mut controls = Vec::new();
            for clip in session
                .arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == track.id && !clip.muted)
            {
                for note in &clip.notes {
                    notes.push((
                        clip.start_tick.0 + note.start_tick.0,
                        note.duration_ticks,
                        note.note,
                        note.velocity,
                    ));
                }
                for event in &clip.instrument_control_events {
                    controls.push((clip.start_tick.0 + event.tick.0, event.kind.clone()));
                }
            }
            notes.sort_unstable();
            controls.sort_by_key(|(tick, kind)| (*tick, format!("{kind:?}")));
            tracks.insert(id.clone(), TrackView { notes, controls });
        }
        let tempo = session
            .arrangement
            .timebase
            .tempo_changes
            .iter()
            .map(|change| (change.tick, change.bpm))
            .collect();
        let meters = session
            .arrangement
            .timebase
            .time_signature_changes
            .iter()
            .map(|change| (change.tick, change.numerator, change.denominator))
            .collect();
        let mut buses = session
            .arrangement
            .mixer
            .buses
            .iter()
            .map(|bus| (bus.name.clone(), bus.kind))
            .collect::<Vec<_>>();
        buses.sort_by_key(|bus| (bus.0.clone(), format!("{:?}", bus.1)));
        PerformanceView {
            tracks,
            tempo,
            meters,
            buses,
        }
    }

    const ARTICULATION_SCORE: &str = r#"{
        "meta":{"title":"Articulations","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"verse","bars":2,"tonic":"C","scale":"ionian"}],
        "sections":{"verse":{
            "lines":{"main":{"instrument":"part","melody":"line"}},
            "events":[
                {"kind":"articulation","instrument":"part","name":"mute","at":{"bar":1,"step":1},"duration_quarters":1},
                {"kind":"articulation","instrument":"part","name":"open","at":{"bar":1,"step":5},"duration_quarters":1},
                {"kind":"articulation","instrument":"part","name":"mute","at":{"bar":2,"step":1},"duration_quarters":1},
                {"kind":"articulation","instrument":"part","name":"open","at":{"bar":2,"step":13},"duration_quarters":1}
            ]
        }},
        "melodies":{"line":["1:16","1:16"]},
        "instruments":{"part":{"role":"Lead role","sound":"Lead sound","source":{
            "vst3":{"plugin":"Synth"},
            "articulations":{
                "default":{"keyswitch":24,"mode":"latch"},
                "mute":{"keyswitch":26,"mode":"latch"},
                "open":{"keyswitch":27,"mode":"hold"},
                "unused":{"keyswitch":28,"mode":"latch"}
            }
        }}}
    }"#;

    const HOLD_ARTICULATION_SCORE: &str = r#"{
        "meta":{"title":"Hold","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"verse","bars":1,"tonic":"C"}],
        "sections":{"verse":{
            "lines":{"main":{"instrument":"part","melody":"line"}},
            "events":[
                {"kind":"articulation","instrument":"part","name":"open","at":{"bar":1,"step":1},"duration_quarters":1}
            ]
        }},
        "melodies":{"line":["1:16"]},
        "instruments":{"part":{"source":{
            "vst3":{"plugin":"Synth"},
            "articulations":{
                "default":{"keyswitch":24,"mode":"latch"},
                "open":{"keyswitch":27,"mode":"hold"},
                "unused":{"keyswitch":28,"mode":"hold"}
            }
        }}}
    }"#;

    fn key_switches(session: &CreativeSession) -> Vec<(u64, u8, bool, String)> {
        let mut sends = Vec::new();
        for track in &session.arrangement.tracks {
            for clip in session
                .arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == track.id)
            {
                for event in &clip.instrument_control_events {
                    if let InstrumentControlEventKind::KeySwitch { note, mode, down } = event.kind {
                        sends.push((
                            clip.start_tick.0 + event.tick.0,
                            note,
                            down,
                            format!("{mode:?}"),
                        ));
                    }
                }
            }
        }
        sends.sort();
        sends
    }

    #[test]
    fn keyswitch_sends_survive_the_round_trip() {
        let root = root("articulations");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            ARTICULATION_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let source_track_id = session.arrangement.tracks[0].id.clone();
        let mut second = session.arrangement.tracks[0].clone();
        second.id = "track:part-2".into();
        second.name = "Part 2".into();
        second
            .instrument
            .as_mut()
            .expect("the source track has an instrument")
            .id = "device:part-2".into();
        session.arrangement.tracks.push(second);
        let clips = session
            .arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == source_track_id)
            .cloned()
            .collect::<Vec<_>>();
        for (index, mut clip) in clips.into_iter().enumerate() {
            clip.id = format!("clip:part-2:{index}");
            clip.track_id = "track:part-2".into();
            session.arrangement.midi_clips.push(clip);
        }

        let first = exported(&session, &root, catalog);
        let text = std::fs::read_to_string(first.path.join("score.yaml")).unwrap();
        crate::score::compile(&text).expect("the exported Score compiles");
        let document = crate::score::parse_document(&text).expect("the document parses");
        for id in ["part", "part_2"] {
            let instrument = &document["instruments"][id];
            assert_eq!(instrument["role"], "Lead role", "{instrument}");
            assert_eq!(instrument["sound"], "Lead sound", "{instrument}");
            let declarations = &instrument["source"]["articulations"];
            assert!(declarations["default"].is_object(), "{declarations}");
            assert!(declarations["mute"].is_object(), "{declarations}");
            assert!(declarations["open"].is_object(), "{declarations}");
            assert!(declarations.get("unused").is_none(), "{declarations}");
        }
        let imported = crate::score_yaml::import::session_from_score(
            &text,
            &root.join("song"),
            &root,
            catalog,
        );
        assert_eq!(key_switches(&session), key_switches(&imported));
    }

    #[test]
    fn hold_articulations_keep_only_their_own_declarations() {
        let root = root("articulations-hold");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let session = crate::score_yaml::import::session_from_score(
            HOLD_ARTICULATION_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let exported = exported(&session, &root, catalog);
        let text = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&text).expect("the document parses");
        let declarations = document["instruments"]["part"]["source"]["articulations"]
            .as_object()
            .expect("the used declaration is kept");
        assert_eq!(
            declarations.keys().collect::<Vec<_>>(),
            ["open"],
            "{declarations:?}"
        );

        let imported = crate::score_yaml::import::session_from_score(
            &text,
            &root.join("song"),
            &root,
            catalog,
        );
        assert_eq!(key_switches(&session), key_switches(&imported));
    }

    #[test]
    fn instruments_without_articulation_events_keep_no_declarations() {
        let root = root("articulations-none");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            HOLD_ARTICULATION_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        for clip in &mut session.arrangement.midi_clips {
            clip.instrument_control_events.clear();
        }
        let exported = exported(&session, &root, catalog);
        let text = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&text).expect("the document parses");
        assert!(
            document["instruments"]["part"]["source"]
                .get("articulations")
                .is_none(),
            "{document}"
        );
    }

    const WALTZ: &str = r#"{
        "meta":{"title":"Waltz","bpm":120,"meter":"3/8","tuning":"12edo","gate":1},
        "form":[{"section":"grid","bars":2,"tonic":"C"},{"section":"hits","bars":2,"tonic":"C"}],
        "sections":{
            "grid":{"drums":"beat"},
            "hits":{
                "drums":"beat",
                "events":[{"kind":"hit","instrument":"kick","at":{"bar":2,"step":1},"velocity":64}]
            }
        },
        "drums":{"beat":{"parts":{"kick":["x...x.","x....."],"snare":["....x.","..x..."]}}},
        "instruments":{
            "kick":{"kit":"drums","trigger_note_midi":36,"role":"Kick role","sound":"Kick sound"},
            "snare":{"kit":"drums","trigger_note_midi":38,"role":"Snare role","sound":"Snare sound"}
        },
        "kits":{"drums":{"source":{"vst3":{"plugin":"Drums"}}}}
    }"#;

    #[test]
    fn unusual_meters_keep_drum_grids_and_duplicate_hits() {
        let root = root("waltz");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let session = crate::score_yaml::import::session_from_score(
            WALTZ,
            &root.join("source"),
            &root,
            catalog,
        );
        let exported = exported(&session, &root, catalog);
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        let grid = &document["sections"]["grid"];
        assert!(grid["drums"].is_string(), "{grid}");
        let hits = &document["sections"]["hits"];
        assert!(hits.get("drums").is_none(), "{hits}");
        assert!(
            hits["events"]
                .as_array()
                .is_some_and(|events| events.iter().all(|event| event["kind"] == "hit")),
            "{hits}"
        );
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        assert_eq!(performance(&session), performance(&imported));
    }

    const CC_SCORE: &str = r#"{
        "meta":{"title":"CC","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"verse","bars":1,"tonic":"C"}],
        "sections":{"verse":{
            "lines":{"main":{"instrument":"lead","melody":"theme"}},
            "events":[{"kind":"automation","instrument":"lead","control":"cc:11","at":{"bar":1,"step":1},"from_value":0,"to_value":127,"duration_quarters":"1/32"}]
        }},
        "melodies":{"theme":["1:16"]},
        "instruments":{"lead":{"source":{"vst3":{"plugin":"Synth"}}}}
    }"#;

    #[test]
    fn cc_automation_round_trips_through_one_ramp() {
        let root = root("cc");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let controls = session.arrangement.midi_clips[0]
            .instrument_control_events
            .iter()
            .filter_map(|event| match event.kind {
                InstrumentControlEventKind::ControlChange { number, value } => {
                    Some((event.tick.0, number, value))
                }
                _ => None,
            })
            .collect::<Vec<_>>();
        assert_eq!(controls.len(), 3, "{controls:?}");

        let exported = exported(&session, &root, catalog);
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        let events = document["sections"]["verse"]["events"]
            .as_array()
            .expect("section events");
        assert!(
            events.iter().any(|event| {
                event["kind"] == "automation"
                    && event["control"] == "cc:11"
                    && event["from_value"] == 0
                    && event["to_value"] == 127
            }),
            "{events:?}"
        );

        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        assert_eq!(performance(&session), performance(&imported));
    }

    #[test]
    fn separate_kit_tracks_keep_their_kits_states_and_mix() {
        use riffra_core::{MidiClip, MidiNote};

        let root = root("multi-kit");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            WALTZ,
            &root.join("source"),
            &root,
            catalog,
        );
        let ppq = u64::from(session.arrangement.timebase.ppq);
        let mut second = session.arrangement.tracks[0].clone();
        second.id = "track:second-kit".into();
        second.name = "Second Kit".into();
        second.gain_db = -6.0;
        second.pan = 0.4;
        let mut instrument = second.instrument.clone().unwrap();
        instrument.id = "device:second-kit".into();
        if let riffra_core::TrackInstrumentSource::Vst3(plugin) = &mut instrument.source {
            plugin.state_data = Some("c2Vjb25k".into());
            plugin.parameter_values = vec![0.5];
        }
        second.instrument = Some(instrument);
        session.arrangement.tracks.push(second);

        let mut third = session.arrangement.tracks[0].clone();
        third.id = "track:clashing-instrument".into();
        third.name = "snare".into();
        third.kind = TrackKind::Instrument;
        third.score_instrument = None;
        let mut instrument = third.instrument.clone().unwrap();
        instrument.id = "device:clashing-instrument".into();
        third.instrument = Some(instrument);
        session.arrangement.tracks.push(third);
        let mut clash_clip = midi_clip("clip:clashing-instrument", "track:clashing-instrument");
        clash_clip.notes.push(MidiNote {
            id: "note:clashing-instrument".into(),
            lyric: None,
            note: 60,
            start_tick: TimelineTick(0),
            duration_ticks: ppq / 4,
            velocity: 100,
            channel: 1,
        });
        session.arrangement.midi_clips.push(clash_clip);
        session.arrangement.midi_clips.push(MidiClip {
            id: "clip:second-kit".into(),
            name: "Second Kit".into(),
            track_id: "track:second-kit".into(),
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: ppq * 4,
            notes: vec![
                MidiNote {
                    id: "note:second-kit".into(),
                    lyric: None,
                    note: 38,
                    start_tick: TimelineTick(ppq),
                    duration_ticks: ppq / 4,
                    velocity: 96,
                    channel: 1,
                },
                MidiNote {
                    id: "note:second-kit-kick".into(),
                    lyric: None,
                    note: 36,
                    start_tick: TimelineTick(ppq),
                    duration_ticks: ppq / 4,
                    velocity: 96,
                    channel: 1,
                },
            ],
            events: Vec::new(),
            instrument_control_events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });

        let exported = exported(&session, &root, catalog);
        assert_eq!(
            exported.score_instruments[&session.arrangement.tracks[0].id],
            "kit:drums"
        );
        assert_eq!(
            exported.score_instruments["track:second-kit"],
            "kit:drums_2"
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        assert!(document["kits"]["drums"].is_object(), "{document}");
        assert!(document["kits"]["drums_2"].is_object(), "{document}");
        assert_eq!(
            document["kits"]["drums_2"]["source"]["vst3"]["state"],
            "states/kits/drums_2/instrument.json"
        );
        assert_eq!(document["instruments"]["kick"]["kit"], "drums");
        assert_eq!(document["instruments"]["kick"]["trigger_note_midi"], 36);
        assert_eq!(document["instruments"]["kick"]["role"], "Kick role");
        assert_eq!(document["instruments"]["kick"]["sound"], "Kick sound");
        assert_eq!(document["instruments"]["snare_2"]["kit"], "drums");
        assert_eq!(document["instruments"]["snare_2"]["trigger_note_midi"], 38);
        assert_eq!(document["instruments"]["snare_2"]["role"], "Snare role");
        assert_eq!(document["instruments"]["kick_2"]["kit"], "drums_2");
        assert_eq!(document["instruments"]["kick_2"]["role"], "Kick role");
        assert_eq!(document["instruments"]["snare_3"]["kit"], "drums_2");
        assert_eq!(document["instruments"]["snare_3"]["trigger_note_midi"], 38);
        assert_eq!(document["instruments"]["snare_3"]["sound"], "Snare sound");
        assert!(document["instruments"]["snare"].is_object());
        assert!(
            document["instruments"]["snare"].get("kit").is_none(),
            "the standalone instrument keeps its own declaration"
        );
        assert!(
            document["instruments"]["snare"].get("role").is_none(),
            "the standalone instrument does not inherit a kit member declaration"
        );
        assert!(
            document["drums"]["grid_drums"]["parts"]
                .get("snare_3")
                .is_some(),
            "{document}"
        );
        assert!(
            exported
                .files
                .contains_key("states/kits/drums_2/instrument.json")
        );
        assert!(
            exported
                .files
                .contains_key("states/kits/drums/instrument.json")
        );

        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        assert_eq!(imported.arrangement.tracks.len(), 3, "{yaml}");
        let second = imported
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("kit:drums_2"))
            .expect("the second kit keeps its own identity");
        assert!((second.gain_db + 6.0).abs() < 1e-9, "{}", second.gain_db);
        assert!((second.pan - 0.4).abs() < 1e-9, "{}", second.pan);
        let instrument = second.instrument.as_ref().unwrap();
        let riffra_core::TrackInstrumentSource::Vst3(plugin) = &instrument.source else {
            panic!("the second kit is a VST3 instrument");
        };
        assert_eq!(plugin.state_data.as_deref(), Some("c2Vjb25k"));
        assert_eq!(plugin.parameter_values, [0.5]);

        let notes = imported
            .arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == second.id)
            .flat_map(|clip| clip.notes.iter())
            .map(|note| (note.note, note.start_tick.0, note.duration_ticks))
            .collect::<Vec<_>>();
        assert_eq!(
            notes,
            [
                (
                    36,
                    imported.arrangement.timebase.ppq.into(),
                    u64::from(imported.arrangement.timebase.ppq) / 4
                ),
                (
                    38,
                    imported.arrangement.timebase.ppq.into(),
                    u64::from(imported.arrangement.timebase.ppq) / 4
                )
            ]
        );
        let clash = imported
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("snare"))
            .expect("the standalone instrument keeps its ID");
        let notes = imported
            .arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == clash.id)
            .flat_map(|clip| clip.notes.iter())
            .map(|note| note.note)
            .collect::<Vec<_>>();
        assert_eq!(notes, [60]);
        assert_ne!(second.id, imported.arrangement.tracks[0].id);
    }

    #[test]
    fn instant_controls_at_the_last_tick_are_written() {
        use riffra_core::{MidiEvent, MidiEventKind};

        let root = root("end-control");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let track = session.arrangement.tracks[0].id.clone();
        session.arrangement.midi_clips.clear();
        session.arrangement.regions.clear();
        let mut clip = midi_clip("clip:end", &track);
        clip.events.push(MidiEvent {
            id: "event:end".into(),
            kind: MidiEventKind::ControlChange,
            tick: TimelineTick(3840),
            channel: 1,
            data1: 11,
            data2: 64,
        });
        session.arrangement.midi_clips.push(clip);

        let exported = exported(&session, &root, catalog);
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        let controls = imported
            .arrangement
            .midi_clips
            .iter()
            .flat_map(|clip| {
                clip.instrument_control_events
                    .iter()
                    .filter_map(|event| match event.kind {
                        InstrumentControlEventKind::ControlChange { number, value } => {
                            Some((clip.start_tick.0 + event.tick.0, number, value))
                        }
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(controls, [(3840, 11, 64)]);
    }

    #[test]
    fn instant_controls_at_the_configured_end_are_written() {
        use riffra_core::{InstrumentControlEvent, InstrumentControlEventKind};

        let root = root("end-configured");
        catalog(&root);
        let catalog = built_in_catalog(&root, r#"{"layers":[],"parameters":[]}"#);
        let mut session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            &catalog,
        );
        session.arrangement.midi_clips.clear();
        session.arrangement.regions.clear();
        session.settings.mixdown.musical_end_tick = 3840;
        let instrument = TrackInstrument::built_in(
            "device:end".into(),
            "End".into(),
            "TEST-001".into(),
            r#"{"layers":[],"parameters":[]}"#.into(),
        )
        .unwrap();
        add_played_track(&mut session, "end", instrument, &[]);
        let clip = session
            .arrangement
            .midi_clips
            .last_mut()
            .expect("the track has a clip");
        clip.instrument_control_events.push(InstrumentControlEvent {
            id: "event:end".into(),
            tick: TimelineTick(3840),
            source_order: 0,
            kind: InstrumentControlEventKind::ModWheel { value: 0.5 },
        });

        let exported = exported(&session, &root, &catalog);
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            &catalog,
        );
        let controls = imported
            .arrangement
            .midi_clips
            .iter()
            .flat_map(|clip| {
                clip.instrument_control_events
                    .iter()
                    .filter_map(|event| match event.kind {
                        InstrumentControlEventKind::ModWheel { value } => {
                            Some((clip.start_tick.0 + event.tick.0, value))
                        }
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(
            controls.len(),
            1,
            "{controls:?}
{yaml}"
        );
        assert_eq!(controls[0].0, 3840);
        assert!((controls[0].1 - 0.5).abs() < 1e-6);
    }

    #[test]
    fn kit_controls_at_the_song_end_are_reported() {
        use riffra_core::{InstrumentControlEvent, InstrumentControlEventKind};

        let root = root("end-kit");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            WALTZ,
            &root.join("source"),
            &root,
            catalog,
        );
        let track = session.arrangement.tracks[0].id.clone();
        let clip = session
            .arrangement
            .midi_clips
            .iter_mut()
            .find(|clip| clip.track_id == track)
            .expect("the kit has a clip");
        clip.instrument_control_events.push(InstrumentControlEvent {
            id: "event:end".into(),
            tick: TimelineTick(2880),
            source_order: 0,
            kind: InstrumentControlEventKind::ControlChange {
                number: 1,
                value: 64,
            },
        });

        let exported = exported(&session, &root, catalog);
        assert!(
            exported.omissions.iter().any(|omission| {
                omission.subject.contains("control") && omission.message.contains("kit track")
            }),
            "{:?}",
            exported.omissions
        );
    }

    #[test]
    fn vst3_raw_midi_cc_keeps_channel_one_and_reports_other_channels() {
        use riffra_core::{MidiEvent, MidiEventKind};

        let root = root("raw-cc-vst3");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let clip = session
            .arrangement
            .midi_clips
            .first_mut()
            .expect("the Score has a clip");
        clip.events = vec![
            MidiEvent {
                id: "event:cc-one".into(),
                kind: MidiEventKind::ControlChange,
                tick: TimelineTick(0),
                channel: 1,
                data1: 7,
                data2: 100,
            },
            MidiEvent {
                id: "event:cc-ten".into(),
                kind: MidiEventKind::ControlChange,
                tick: TimelineTick(480),
                channel: 10,
                data1: 7,
                data2: 64,
            },
        ];

        let exported = exported(&session, &root, catalog);
        assert!(
            exported.omissions.iter().any(|omission| {
                omission.subject.contains("MIDI CC")
                    && omission.subject.contains("tick 480")
                    && omission.message.contains("channel 10")
            }),
            "{:?}",
            exported.omissions
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        assert!(yaml.contains("number: 7"), "{yaml}");
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        let controls = imported
            .arrangement
            .midi_clips
            .iter()
            .flat_map(|clip| {
                clip.instrument_control_events
                    .iter()
                    .filter_map(|event| match event.kind {
                        InstrumentControlEventKind::ControlChange { number: 7, value } => {
                            Some((clip.start_tick.0 + event.tick.0, value))
                        }
                        _ => None,
                    })
            })
            .collect::<Vec<_>>();
        assert_eq!(controls, [(0, 100), (480, 64)]);
    }

    #[test]
    fn built_in_raw_midi_cc_is_reported_and_notes_survive() {
        use riffra_core::{MidiEvent, MidiEventKind};

        let root = root("raw-cc-built-in");
        catalog(&root);
        let catalog = built_in_catalog(
            &root,
            r#"{"layers":[{"id":"main","trigger":{"event":"note_on","key_min":60,"key_max":60,"velocity_min":1,"velocity_max":127}}]}"#,
        );
        let mut session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            crate::test_support::empty_built_in_catalog(),
        );
        let instrument = TrackInstrument::built_in(
            "device:test".into(),
            "Test".into(),
            "TEST-001".into(),
            r#"{"layers":[{"id":"main","trigger":{"event":"note_on","key_min":60,"key_max":60,"velocity_min":1,"velocity_max":127}}]}"#.into(),
        )
        .unwrap();
        let track = add_played_track(&mut session, "test", instrument, &[60]);
        let clip = session
            .arrangement
            .midi_clips
            .iter_mut()
            .find(|clip| clip.track_id == track)
            .expect("the built-in track has a clip");
        clip.events.push(MidiEvent {
            id: "event:cc".into(),
            kind: MidiEventKind::ControlChange,
            tick: TimelineTick(0),
            channel: 1,
            data1: 7,
            data2: 100,
        });

        let exported = exported(&session, &root, &catalog);
        assert!(
            exported.omissions.iter().any(|omission| {
                omission.subject.contains("MIDI CC")
                    && omission.message.contains("built-in instrument")
            }),
            "{:?}",
            exported.omissions
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        assert!(!yaml.contains("cc:7"), "{yaml}");
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            &catalog,
        );
        let imported_track = imported
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("test"))
            .expect("the built-in track survives");
        let notes = imported
            .arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == imported_track.id)
            .flat_map(|clip| clip.notes.iter())
            .map(|note| note.note)
            .collect::<Vec<_>>();
        assert_eq!(notes, [60]);
        let controls = imported
            .arrangement
            .midi_clips
            .iter()
            .filter(|clip| clip.track_id == imported_track.id)
            .flat_map(|clip| clip.instrument_control_events.iter())
            .filter(|event| {
                matches!(
                    event.kind,
                    InstrumentControlEventKind::ControlChange { number: 7, .. }
                )
            })
            .count();
        assert_eq!(controls, 0);
    }

    #[test]
    fn non_default_channels_are_reported() {
        let root = root("channels");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let clip = session
            .arrangement
            .midi_clips
            .first_mut()
            .expect("the Score has a clip");
        clip.notes.push(MidiNote {
            id: "note:channel-one".into(),
            lyric: None,
            note: 67,
            start_tick: TimelineTick(480),
            duration_ticks: 480,
            velocity: 100,
            channel: 1,
        });
        let pitches = clip.notes.iter().map(|note| note.note).collect::<Vec<_>>();
        clip.notes[0].channel = 10;

        let exported = exported(&session, &root, catalog);
        assert!(
            exported.omissions.iter().any(|omission| {
                omission.subject.contains("note") && omission.message.contains("channel 10")
            }),
            "{:?}",
            exported.omissions
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        let pitches_after = imported.arrangement.midi_clips[0]
            .notes
            .iter()
            .map(|note| note.note)
            .collect::<Vec<_>>();
        assert_eq!(pitches_after, pitches);
    }

    #[test]
    fn built_in_notes_use_the_catalog_definition() {
        let root = root("built-in-rules");
        catalog(&root);
        let catalog = built_in_catalog(
            &root,
            r#"{"layers":[{"id":"main","trigger":{"event":"note_on","key_min":60,"key_max":60,"velocity_min":1,"velocity_max":127}}]}"#,
        );
        let mut session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            crate::test_support::empty_built_in_catalog(),
        );
        let instrument = TrackInstrument::built_in(
            "device:test".into(),
            "Test".into(),
            "TEST-001".into(),
            r#"{"layers":[{"id":"main","trigger":{"event":"note_on","key_min":48,"key_max":72,"velocity_min":1,"velocity_max":127}}]}"#.into(),
        )
        .unwrap();
        add_played_track(&mut session, "test", instrument, &[60, 64]);

        let exported = exported(&session, &root, &catalog);
        assert!(
            exported.omissions.iter().any(|omission| {
                omission.subject.contains("note 64") && omission.message.contains("cannot trigger")
            }),
            "{:?}",
            exported.omissions
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            &catalog,
        );
        let notes = imported
            .arrangement
            .midi_clips
            .iter()
            .filter(|clip| {
                imported.arrangement.tracks.iter().any(|candidate| {
                    candidate.id == clip.track_id
                        && candidate.score_instrument.as_deref() == Some("test")
                })
            })
            .flat_map(|clip| clip.notes.iter())
            .map(|note| note.note)
            .collect::<Vec<_>>();
        assert_eq!(notes, [60]);
    }

    #[test]
    fn unreadable_instrument_definitions_fail_the_export() {
        let root = root("invalid-definition");
        catalog(&root);
        let snapshot = "00000000-0000-4000-8000-000000000002";
        let package = root.join("project-instruments").join(snapshot);
        std::fs::create_dir_all(&package).unwrap();
        std::fs::write(package.join("definition.json"), "not a definition").unwrap();
        let mut session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            crate::test_support::empty_built_in_catalog(),
        );
        let instrument = TrackInstrument::user_snapshot(
            "device:broken".into(),
            "Broken".into(),
            format!("user:{snapshot}"),
            snapshot.into(),
            r#"{"layers":[]}"#.into(),
        )
        .unwrap();
        add_played_track(&mut session, "broken", instrument, &[]);

        let error = match plan(
            &session,
            &root.join("song"),
            &root,
            crate::test_support::empty_built_in_catalog(),
            Path::new("unused"),
        ) {
            Ok(_) => panic!("the invalid definition must fail the export"),
            Err(error) => error,
        };
        assert!(
            error.message.contains("definition is invalid"),
            "{}",
            error.message
        );
        assert!(!root.join("song").exists());
    }

    #[test]
    fn user_snapshot_notes_use_the_copied_definition() {
        let root = root("user-rules");
        catalog(&root);
        let snapshot = "00000000-0000-4000-8000-000000000001";
        let package = root.join("project-instruments").join(snapshot);
        std::fs::create_dir_all(&package).unwrap();
        let definition = r#"{"layers":[{"id":"main","trigger":{"event":"note_on","key_min":60,"key_max":60,"velocity_min":1,"velocity_max":127}}]}"#;
        std::fs::write(package.join("definition.json"), definition).unwrap();
        std::fs::create_dir_all(package.join("samples")).unwrap();
        std::fs::write(package.join("samples/raw.bin"), b"sample payload").unwrap();
        let mut session = crate::score_yaml::import::session_from_score(
            CC_SCORE,
            &root.join("source"),
            &root,
            crate::test_support::empty_built_in_catalog(),
        );
        let instrument = TrackInstrument::user_snapshot(
            "device:user".into(),
            "User".into(),
            format!("user:{snapshot}"),
            snapshot.into(),
            r#"{"layers":[{"id":"main","trigger":{"event":"note_on","key_min":48,"key_max":72,"velocity_min":1,"velocity_max":127}}]}"#.into(),
        )
        .unwrap();
        add_played_track(&mut session, "user", instrument, &[60, 64]);

        let catalog = crate::test_support::empty_built_in_catalog();
        let exported = exported(&session, &root, catalog);
        assert!(
            exported.omissions.iter().any(|omission| {
                omission.subject.contains("note 64") && omission.message.contains("cannot trigger")
            }),
            "{:?}",
            exported.omissions
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        assert_eq!(
            document["instruments"]["user"]["definition"],
            "sounds/user/definition.json"
        );
        assert_eq!(
            std::fs::read_to_string(exported.path.join("sounds/user/definition.json")).unwrap(),
            definition
        );
        assert_eq!(
            std::fs::read(exported.path.join("sounds/user/samples/raw.bin")).unwrap(),
            b"sample payload"
        );
    }

    #[test]
    fn a_missing_audio_source_fails_the_write_and_removes_the_staging_folder() {
        let root = root("missing-audio");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let source = root.join("source");
        std::fs::create_dir_all(source.join("audio")).unwrap();
        let wav = source.join("audio/rain.wav");
        std::fs::write(&wav, one_sample_wav()).unwrap();
        let session =
            crate::score_yaml::import::session_from_score(AUDIO_SCORE, &source, &root, catalog);
        let destination = root.join("song");
        let plan = plan(&session, &destination, &root, catalog, Path::new("unused")).expect("plan");
        let location = crate::asset::resolve_content_location(
            &root,
            &session.arrangement.audio_clips[0].asset_id,
        )
        .unwrap()
        .expect("the imported asset has a content location");
        std::fs::remove_file(location).unwrap();

        let error = plan.write().unwrap_err();
        assert!(
            error.message.contains("could not copy"),
            "{}",
            error.message
        );
        assert!(!destination.exists());
        let staging = std::fs::read_dir(&root)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|entry| {
                entry
                    .file_name()
                    .to_string_lossy()
                    .starts_with(".riffra-export-")
            })
            .count();
        assert_eq!(staging, 0, "the failed staging folder must be removed");
    }

    #[test]
    fn unassigned_kit_tracks_are_reported_and_not_exported() {
        let root = root("unassigned-kit");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let kit = session
            .arrangement
            .tracks
            .iter_mut()
            .find(|track| {
                track
                    .score_instrument
                    .as_deref()
                    .is_some_and(|id| id.starts_with("kit:"))
            })
            .expect("the Score has a kit track");
        kit.instrument = None;
        let kit_name = kit.name.clone();

        let exported = exported(&session, &root, catalog);
        assert!(
            exported.omissions.iter().any(|omission| {
                omission.subject.contains(&kit_name)
                    && omission
                        .message
                        .contains("performance content but no instrument")
            }),
            "{:?}",
            exported.omissions
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        crate::score::compile(&yaml).expect("the exported Score compiles");
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        assert!(document.get("drums").is_none(), "{document}");
        assert!(document["instruments"].get("lead").is_some(), "{document}");
        let imported =
            crate::score_yaml::import::session_from_score(&yaml, &exported.path, &root, catalog);
        let lead = imported
            .arrangement
            .tracks
            .iter()
            .find(|track| track.score_instrument.as_deref() == Some("lead"))
            .expect("the normal instrument track survives");
        assert!(
            imported
                .arrangement
                .midi_clips
                .iter()
                .filter(|clip| clip.track_id == lead.id)
                .any(|clip| !clip.notes.is_empty()),
            "the normal instrument keeps its notes"
        );
    }

    #[test]
    fn harmony_gaps_survive_the_round_trip() {
        let root = root("harmony-gaps");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        assert!(session.arrangement.harmony_events.len() >= 2);
        session.arrangement.harmony_events[0].end_tick = TimelineTick(960);
        session.arrangement.harmony_events[1].end_tick = TimelineTick(5760);
        let expected = session
            .arrangement
            .harmony_events
            .iter()
            .map(|event| {
                (
                    event.start_tick.0,
                    event.end_tick.0,
                    event.chord.root.map(|root| i32::from(root.pitch_class())),
                    event.chord.bass.map(|bass| i32::from(bass.pitch_class())),
                    event
                        .chord
                        .tones
                        .iter()
                        .map(|tone| i32::from(tone.pitch_class()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();

        let exported = exported(&session, &root, catalog);
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        let changes = document["sections"]["verse"]["chord_changes"]
            .as_array()
            .expect("the verse has explicit changes");
        assert_eq!(
            changes
                .iter()
                .map(|change| change["chord"].as_str().unwrap())
                .collect::<Vec<_>>(),
            ["1:maj", "NC", "6:m7", "NC"]
        );

        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        let actual = imported
            .arrangement
            .harmony_events
            .iter()
            .filter(|event| event.start_tick.0 < 7680)
            .map(|event| {
                (
                    event.start_tick.0,
                    event.end_tick.0,
                    event.chord.root.map(|root| i32::from(root.pitch_class())),
                    event.chord.bass.map(|bass| i32::from(bass.pitch_class())),
                    event
                        .chord
                        .tones
                        .iter()
                        .map(|tone| i32::from(tone.pitch_class()))
                        .collect::<Vec<_>>(),
                )
            })
            .collect::<Vec<_>>();
        assert_eq!(actual, expected);
    }

    const AUDIO_SCORE: &str = r#"{
        "meta":{"title":"Audio","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"quiet","bars":2,"tonic":"C"},{"section":"loud","bars":2,"tonic":"C"}],
        "sections":{
            "quiet":{
                "lines":{"main":{"instrument":"lead","melody":"theme"}},
                "audio_clips":[{"id":"rain","instrument":"ambience","file":"audio/rain.wav","at":{"bar":1,"step":1},"gain_db":-6,"pan":0.25}]
            },
            "loud":{
                "lines":{"main":{"instrument":"lead","melody":"theme"}},
                "audio_clips":[{"id":"rain","instrument":"ambience","file":"audio/rain.wav","at":{"bar":2,"step":1},"gain_db":-6,"pan":0.25}]
            }
        },
        "melodies":{"theme":["1:16","1:16"]},
        "instruments":{"lead":{"source":{"vst3":{"plugin":"Synth"}}},"ambience":{"audio":true}},
        "mix":{
            "buses":{"main":{"kind":"group"},"room":{"kind":"return"}},
            "tracks":{"ambience":{"output":"main","sends":{"room":-6}}}
        }
    }"#;

    fn one_sample_wav() -> Vec<u8> {
        let mut wav = Vec::from(&b"RIFF"[..]);
        wav.extend_from_slice(&40_u32.to_le_bytes());
        wav.extend_from_slice(b"WAVEfmt ");
        wav.extend_from_slice(&16_u32.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&1_u16.to_le_bytes());
        wav.extend_from_slice(&44_100_u32.to_le_bytes());
        wav.extend_from_slice(&88_200_u32.to_le_bytes());
        wav.extend_from_slice(&2_u16.to_le_bytes());
        wav.extend_from_slice(&16_u16.to_le_bytes());
        wav.extend_from_slice(b"data");
        wav.extend_from_slice(&2_u32.to_le_bytes());
        wav.extend_from_slice(&[0, 0]);
        wav
    }

    #[test]
    fn audio_clips_are_unique_and_keep_sections_apart() {
        let root = root("audio");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let source = root.join("source");
        std::fs::create_dir_all(source.join("audio")).unwrap();
        std::fs::write(source.join("audio/rain.wav"), one_sample_wav()).unwrap();
        let session =
            crate::score_yaml::import::session_from_score(AUDIO_SCORE, &source, &root, catalog);
        assert_eq!(session.arrangement.audio_clips.len(), 2);
        let exported = exported(&session, &root, catalog);
        assert!(
            !exported
                .omissions
                .iter()
                .any(|omission| omission.subject.contains("playback length")),
            "{:?}",
            exported.omissions
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        let mut ids = BTreeSet::new();
        for section in document["sections"].as_object().expect("sections") {
            if let Some(clips) = section.1.get("audio_clips").and_then(Value::as_array) {
                for clip in clips {
                    ids.insert(clip["id"].as_str().expect("clip id").to_owned());
                }
            }
        }
        assert_eq!(ids.len(), 2, "{ids:?}");
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        let clips = &imported.arrangement.audio_clips;
        assert_eq!(clips.len(), 2);
        let mut starts = clips
            .iter()
            .map(|clip| {
                assert_eq!(clip.source_range.start, 0);
                assert_eq!(clip.source_range.end, 1);
                assert!((clip.gain_db + 6.0).abs() < 1e-9);
                assert!((clip.pan - 0.25).abs() < 1e-9);
                clip.start_tick.0
            })
            .collect::<Vec<_>>();
        starts.sort_unstable();
        assert_eq!(starts, [0, 5760]);
        let wavs = exported
            .files
            .keys()
            .filter(|path| path.starts_with("audio/"))
            .collect::<Vec<_>>();
        assert_eq!(wavs.len(), 1, "one WAV is shared by both clips: {wavs:?}");
        assert_eq!(
            std::fs::read(exported.path.join(wavs[0])).unwrap(),
            one_sample_wav(),
            "the exported WAV keeps the source bytes"
        );
        let ambience = imported
            .arrangement
            .tracks
            .iter()
            .find(|track| track.name == "ambience")
            .expect("audio track");
        let output = imported
            .arrangement
            .mixer
            .buses
            .iter()
            .find(|bus| bus.name == "main")
            .expect("main bus");
        assert_eq!(ambience.output_bus_id.as_deref(), Some(output.id.as_str()));
        let room = imported
            .arrangement
            .mixer
            .buses
            .iter()
            .find(|bus| bus.name == "room")
            .expect("room bus");
        let send = imported
            .arrangement
            .mixer
            .sends
            .iter()
            .find(|send| send.track_id == ambience.id)
            .expect("ambience send");
        assert_eq!(send.return_bus_id, room.id);
        assert!((send.level_db + 6.0).abs() < 1e-9);
    }

    #[test]
    fn audio_clips_with_a_different_playback_length_are_reported() {
        use riffra_core::{FrameDuration, FrameRange};

        let root = root("audio-length");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let source = root.join("source");
        std::fs::create_dir_all(source.join("audio")).unwrap();
        std::fs::write(source.join("audio/rain.wav"), one_sample_wav()).unwrap();
        let mut session =
            crate::score_yaml::import::session_from_score(AUDIO_SCORE, &source, &root, catalog);
        let clip = session
            .arrangement
            .audio_clips
            .first_mut()
            .expect("the Score has an audio clip");
        clip.source_sample_rate = 48_000;
        clip.source_range = FrameRange {
            start: 0,
            end: 48_000,
        };
        clip.timeline_duration = FrameDuration {
            frames: 96_000,
            sample_rate: 48_000,
        };

        let exported = exported(&session, &root, catalog);
        assert!(
            exported.omissions.iter().any(|omission| {
                omission.subject.contains("ambience")
                    && omission.subject.contains("rain")
                    && omission.subject.contains("playback length")
                    && omission
                        .message
                        .contains("different length than its source")
            }),
            "{:?}",
            exported.omissions
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        let clips = document["sections"]
            .as_object()
            .expect("sections")
            .values()
            .filter_map(|section| section.get("audio_clips").and_then(Value::as_array))
            .flatten()
            .collect::<Vec<_>>();
        assert_eq!(
            clips.len(),
            2,
            "the clip is still written from its source range"
        );
    }

    const STATE_SCORE: &str = r#"{
        "meta":{"title":"States","bpm":120,"meter":"4/4","tuning":"12edo","gate":1},
        "form":[{"section":"verse","bars":1,"tonic":"C"}],
        "sections":{"verse":{"lines":{
            "main":{"instrument":"lead","melody":"theme"},
            "bass":{"instrument":"bass","melody":"bass_line","register":"bass"}
        }}},
        "melodies":{"theme":["1:16"],"bass_line":["1:16"]},
        "instruments":{
            "lead":{
                "source":{"vst3":{"plugin":"Synth"}},
                "effects":[{"id":"lead-eq","vst3":{"plugin":"EQ"}}]
            },
            "bass":{"source":{"vst3":{"plugin":"Bass"}}}
        },
        "mix":{
            "buses":{
                "lead":{"kind":"group","effects":[{"id":"lead-bus-eq","vst3":{"plugin":"EQ"}}]},
                "drum_bus":{"kind":"group","effects":[{"id":"bus-eq","vst3":{"plugin":"EQ"}}]},
                "room":{"kind":"return"}
            },
            "tracks":{"lead":{"output":"lead","sends":{"room":-12}}},
            "automation":[
                {"target":{"bus":"lead","parameter":"gain"},"section":"verse","points":[{"at":{"bar":1,"step":1},"value":-6}]}
            ]
        }
    }"#;

    #[test]
    fn state_files_are_scoped_by_owner_kind() {
        let root = root("states");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let session = crate::score_yaml::import::session_from_score(
            STATE_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let exported = exported(&session, &root, catalog);
        let states = exported
            .files
            .keys()
            .filter(|path| path.starts_with("states/"))
            .cloned()
            .collect::<Vec<_>>();
        assert!(
            states.contains(&"states/instruments/lead/instrument.json".into()),
            "{states:?}"
        );
        assert!(
            states.contains(&"states/instruments/lead/effects/eq.json".into()),
            "{states:?}"
        );
        assert!(
            states.contains(&"states/buses/lead/effects/eq.json".into()),
            "{states:?}"
        );
        assert!(
            states.contains(&"states/buses/drum_bus/effects/eq.json".into()),
            "{states:?}"
        );
        let yaml = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        crate::score::compile(&yaml).expect("the exported Score compiles");
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            catalog,
        );
        let track = imported
            .arrangement
            .tracks
            .iter()
            .find(|track| track.name == "lead")
            .expect("lead track");
        let room = imported
            .arrangement
            .mixer
            .buses
            .iter()
            .find(|bus| bus.name == "room")
            .expect("room bus");
        let send = imported
            .arrangement
            .mixer
            .sends
            .iter()
            .find(|send| send.track_id == track.id)
            .expect("lead send");
        assert_eq!(send.return_bus_id, room.id);
        assert!((send.level_db + 12.0).abs() < 1e-9);
        let bass = imported
            .arrangement
            .tracks
            .iter()
            .find(|track| track.name == "bass")
            .expect("bass track");
        let drum_bus = imported
            .arrangement
            .mixer
            .buses
            .iter()
            .find(|bus| bus.name == "drum_bus")
            .expect("drum bus");
        let bus_effect = drum_bus
            .effects
            .iter()
            .find(|effect| effect.name == "EQ")
            .expect("bus effect");
        let mut routed = imported.clone();
        routed
            .arrangement
            .mixer
            .buses
            .iter_mut()
            .find(|bus| bus.name == "drum_bus")
            .unwrap()
            .name = "Drum Bus".into();
        routed
            .arrangement
            .mixer
            .sidechain_sends
            .push(riffra_core::SidechainSend {
                id: "score:sidechain:test".into(),
                source_track_id: bass.id.clone(),
                target_device_id: bus_effect.id.clone(),
                level_db: -3.0,
                position: riffra_core::SendPosition::PostFader,
                enabled: true,
            });
        let routed_plan = plan(
            &routed,
            &root.join("routed"),
            &root,
            catalog,
            Path::new("unused"),
        )
        .expect("routed export");
        routed_plan.write().expect("write");
        let yaml = std::fs::read_to_string(root.join("routed/score.yaml")).unwrap();
        let document = crate::score::parse_document(&yaml).expect("the document parses");
        let route = &document["mix"]["sidechain"][0];
        assert_eq!(route["from"], "bass");
        assert_eq!(route["to"]["bus"], "drum_bus", "{route}");
        assert_eq!(route["to"]["effect"], "eq", "{route}");
        assert_eq!(route["level_db"], -3.0);
        assert!(
            imported
                .arrangement
                .automation_lanes
                .iter()
                .any(|lane| matches!(lane.target, riffra_core::AutomationTarget::BusMix { .. })),
            "the bus gain automation is kept"
        );
    }

    #[test]
    fn notes_outside_the_instrument_triggers_are_reported_and_not_exported() {
        use riffra_core::{MidiClip, MidiNote, Track, TrackInstrument};

        let root = root("trigger-range");
        let definition = r#"{"metadata":{"name":"Range"},"layers":[{"id":"main","trigger":{"event":"note_on","key_min":60,"key_max":72,"velocity_min":1,"velocity_max":100}}]}"#;
        let catalog = built_in_catalog(&root, definition);
        let mut session = CreativeSession::new(1);
        let mut track = Track::instrument("track:lead".into(), "Lead".into());
        track.instrument = Some(
            TrackInstrument::built_in(
                "device:lead".into(),
                "Range".into(),
                "TEST-001".into(),
                definition.into(),
            )
            .unwrap(),
        );
        let track_id = track.id.clone();
        session.arrangement.tracks.push(track);
        session.arrangement.midi_clips.push(MidiClip {
            id: "clip:lead".into(),
            name: "Lead".into(),
            track_id,
            asset_id: None,
            start_tick: TimelineTick(0),
            duration_ticks: 1920,
            notes: vec![
                MidiNote {
                    id: "note:in-range".into(),
                    lyric: None,
                    note: 64,
                    start_tick: TimelineTick(0),
                    duration_ticks: 480,
                    velocity: 100,
                    channel: 1,
                },
                MidiNote {
                    id: "note:out-of-range".into(),
                    lyric: None,
                    note: 40,
                    start_tick: TimelineTick(480),
                    duration_ticks: 480,
                    velocity: 100,
                    channel: 1,
                },
                MidiNote {
                    id: "velocity-out-of-range".into(),
                    lyric: None,
                    note: 72,
                    start_tick: TimelineTick(960),
                    duration_ticks: 480,
                    velocity: 120,
                    channel: 1,
                },
            ],
            events: Vec::new(),
            instrument_control_events: Vec::new(),
            muted: false,
            loop_enabled: false,
            recording_take_id: None,
        });

        let plan = plan(
            &session,
            &root.join("song"),
            &root,
            &catalog,
            Path::new("unused"),
        )
        .expect("export");
        assert!(
            plan.omissions.iter().any(|omission| {
                omission.subject.contains("note 40") && omission.message.contains("cannot trigger")
            }),
            "{:?}",
            plan.omissions
        );
        plan.write().expect("write");
        let yaml = std::fs::read_to_string(root.join("song/score.yaml")).unwrap();
        crate::score::compile(&yaml).expect("the exported Score compiles");
        let imported = crate::score_yaml::import::session_from_score(
            &yaml,
            &root.join("song"),
            &root,
            &catalog,
        );
        let notes = imported
            .arrangement
            .midi_clips
            .iter()
            .flat_map(|clip| clip.notes.iter())
            .collect::<Vec<_>>();
        assert_eq!(notes.len(), 1, "{yaml}");
        assert_eq!(notes[0].note, 64);
        assert_eq!(notes[0].velocity, 100);
        assert_eq!(notes[0].start_tick.0, 0);
        assert_eq!(
            notes[0].duration_ticks * 2,
            u64::from(imported.arrangement.timebase.ppq)
        );
    }

    #[test]
    fn existing_destinations_are_refused_without_writing() {
        let root = root("existing");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let session = crate::score_yaml::import::session_from_score(
            SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let destination = root.join("taken");
        std::fs::create_dir_all(&destination).unwrap();
        let error = plan(&session, &destination, &root, catalog, Path::new("unused"))
            .err()
            .expect("existing destination");
        assert!(
            error.message.contains("already exists"),
            "{}",
            error.message
        );
        assert!(std::fs::read_dir(&destination).unwrap().next().is_none());
    }

    #[test]
    fn muted_clips_are_reported_and_the_output_stays_readable() {
        let root = root("omissions");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        session.arrangement.midi_clips[0].muted = true;
        let exported = exported(&session, &root, catalog);
        assert!(
            exported
                .omissions
                .iter()
                .any(|omission| omission.subject.contains("clip")),
            "{:?}",
            exported.omissions
        );
        assert!(
            exported
                .omissions
                .iter()
                .all(|omission| omission.subject.contains("clip")
                    || omission
                        .message
                        .contains("evaluates this volume automation linearly")),
            "{:?}",
            exported.omissions
        );
        let text = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        crate::score::compile(&text).expect("the exported Score compiles");
    }

    #[test]
    fn mixdown_settings_the_score_cannot_express_are_reported() {
        let root = root("mixdown-omission");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        session.settings.mixdown.tail_seconds = 2.0;

        let exported = exported(&session, &root, catalog);

        assert!(
            exported
                .omissions
                .iter()
                .any(|omission| omission.subject == "mixdown"),
            "{:?}",
            exported.omissions
        );
    }

    #[test]
    fn mismatched_regions_fall_back_to_one_song_section() {
        let root = root("regions");
        catalog(&root);
        let catalog = crate::test_support::empty_built_in_catalog();
        let mut session = crate::score_yaml::import::session_from_score(
            SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        session.arrangement.regions[1].start_tick = TimelineTick(1441);
        let exported = exported(&session, &root, catalog);
        assert!(
            exported
                .omissions
                .iter()
                .any(|omission| omission.subject == "regions"),
            "{:?}",
            exported.omissions
        );
        let text = std::fs::read_to_string(exported.path.join("score.yaml")).unwrap();
        assert!(text.contains("section: song"), "{text}");
    }
}
