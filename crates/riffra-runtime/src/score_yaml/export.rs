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
        let complete = self.full_bar_ticks(ppq, self.bars().saturating_sub(1));
        (last < complete).then(|| quarters(last, ppq))
    }

    fn full_bar_ticks(&self, ppq: u32, _index: usize) -> u64 {
        let (numerator, denominator) = self.meter;
        u64::from(numerator) * 4 * u64::from(ppq) / u64::from(denominator)
    }

    /// Converts an absolute tick to a Score position inside this section.
    pub(super) fn position(&self, tick: u64, ppq: u32) -> Value {
        position_in(
            &self.bar_starts,
            &self.bar_lengths,
            self.start_tick,
            tick,
            ppq,
        )
    }
}

pub(super) fn position_in(
    bar_starts: &[u64],
    bar_lengths: &[u64],
    section_start: u64,
    tick: u64,
    ppq: u32,
) -> Value {
    let relative = tick.saturating_sub(section_start);
    let index = bar_starts.partition_point(|start| *start <= tick).max(1) - 1;
    let bar = index + 1;
    let offset = tick.saturating_sub(bar_starts[index]);
    let steps = i128::from(offset) * 4;
    if steps % i128::from(ppq) == 0 {
        json!({"bar": bar, "step": u64::try_from(steps / i128::from(ppq)).unwrap_or(0) + 1})
    } else {
        let _ = (bar_lengths, relative);
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
}

/// Original Score declarations the Session cannot reconstruct.
pub(super) struct Original {
    document: Value,
    plan: score::ScorePlan,
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
    pub(super) original: Option<Original>,
    pub(super) omissions: Vec<ScoreExportOmission>,
    /// Files of the song folder by their relative path.
    pub(super) files: BTreeMap<String, Vec<u8>>,
    /// Cached plugin parameter names by device id.
    pub(super) parameters: BTreeMap<String, Vec<PluginParameter>>,
}

/// One automatable plugin parameter reported by the scanner.
pub(super) struct PluginParameter {
    pub(super) id: Option<String>,
    pub(super) name: String,
    pub(super) index: u32,
}

/// A song folder assembled in memory.
pub(crate) struct ExportPlan {
    pub(crate) path: PathBuf,
    pub(crate) omissions: Vec<ScoreExportOmission>,
    /// Score instrument ID written for each canonical Track.
    pub(crate) score_instruments: BTreeMap<String, String>,
    files: BTreeMap<String, Vec<u8>>,
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
    let original = original(session);
    let clock = Clock::new(&session.arrangement.timebase);
    let song_end = song_end(session, &clock);
    let mut omissions = Vec::new();
    let sections = clock.sections(session, song_end, original.as_ref(), &mut omissions)?;
    let tracks = resolve_tracks(session, &mut omissions);
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
        parameters: BTreeMap::new(),
    };
    export.kits = kit_members(&export);
    let mut document = Map::new();
    performance::apply(&mut export, &mut document)?;
    production::apply(&mut export, &mut document)?;
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
        score: emit::yaml(&Value::Object(document)),
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

fn original(session: &CreativeSession) -> Option<Original> {
    let source = session.score_source.as_ref()?;
    let plan = score::compile(&source.yaml).ok()?;
    let document = score::parse_document(&source.yaml).ok()?;
    Some(Original { document, plan })
}

impl Original {
    /// Returns the Score form occurrence matching one region.
    pub(super) fn occurrence(
        &self,
        index: usize,
        name: &str,
        start_tick: u64,
        ppq: u32,
    ) -> Option<&score::Occurrence> {
        let occurrence = self.plan.sections.get(index)?;
        let expected = ticks(
            &to_quarters(occurrence.start_tick, u32::from(self.plan.ticks_per_beat)),
            ppq,
        )?;
        (occurrence.section == name && expected == start_tick).then_some(occurrence)
    }

    /// Returns a raw Score field, following no inheritance.
    pub(super) fn field(&self, path: &[&str]) -> Option<&Value> {
        let mut value = &self.document;
        for key in path {
            value = value.get(key)?;
        }
        Some(value)
    }

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
fn content_end(session: &CreativeSession, clock: &Clock) -> u64 {
    let mut end = 0;
    for clip in &session.arrangement.midi_clips {
        if clip.muted {
            continue;
        }
        for note in &clip.notes {
            end = end.max(clip.start_tick.0 + note.start_tick.0 + note.duration_ticks);
        }
        for event in clip.events.iter().map(|event| event.tick.0).chain(
            clip.instrument_control_events.iter().map(|event| {
                event.tick.0
                    + match event.kind {
                        riffra_core::InstrumentControlEventKind::ParameterRamp {
                            duration_ticks,
                            ..
                        } => duration_ticks,
                        _ => 0,
                    }
            }),
        ) {
            end = end.max(clip.start_tick.0 + event);
        }
    }
    for clip in &session.arrangement.audio_clips {
        if clip.muted {
            continue;
        }
        end = end.max(clip.start_tick.0 + clip.timeline_duration.frames * u64::from(clock.ppq) * 2);
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
    let content = content_end(session, clock);
    let configured = session.settings.mixdown.musical_end_tick;
    let end = if configured == 0 {
        if content == 0 {
            clock.bar_ticks(0).max(1)
        } else {
            clock.bar_end(content.saturating_sub(1))
        }
    } else {
        configured.max(content)
    };
    if clock.is_bar_start(end) {
        end
    } else {
        let start = clock.bar_start(end.saturating_sub(1));
        let bar = clock.bar_ticks(start);
        let minimum = start + bar / 4;
        end.max(minimum)
    }
}

impl Clock {
    /// Resolves the arrangement's regions into Score sections.
    fn sections(
        &self,
        session: &CreativeSession,
        end: u64,
        original: Option<&Original>,
        omissions: &mut Vec<ScoreExportOmission>,
    ) -> Result<Vec<SongSection>, ExportError> {
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
        for (base, start, stop) in spans {
            for (start, stop) in self.split_meters(start, stop) {
                sections.push(self.section(
                    &base,
                    start,
                    stop,
                    original,
                    sections.len(),
                    omissions,
                )?);
            }
        }
        Ok(sections)
    }

    /// Splits a span at meter changes inside it.
    fn split_meters(&self, start: u64, end: u64) -> Vec<(u64, u64)> {
        let mut result = Vec::new();
        let mut cursor = start;
        let changes = self
            .meters
            .iter()
            .filter(|(at, ..)| *at > start && *at < end)
            .copied()
            .collect::<Vec<_>>();
        for change in &changes {
            let mut at = change.0;
            while !self.is_bar_start(at) {
                at = self.bar_end(at);
            }
            if at > cursor && at < end {
                result.push((cursor, at));
                cursor = at;
            }
        }
        if cursor < end {
            result.push((cursor, end));
        }
        result
    }

    fn section(
        &self,
        base: &str,
        start: u64,
        end: u64,
        original: Option<&Original>,
        index: usize,
        omissions: &mut Vec<ScoreExportOmission>,
    ) -> Result<SongSection, ExportError> {
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
        if let Some(original) = original {
            let region_start = self
                .meters
                .iter()
                .find(|(at, ..)| *at == start || *at <= start)
                .map_or(start, |_| start);
            if let Some(occurrence) = original.occurrence(index, base, region_start, self.ppq) {
                tonics = (occurrence.tonic.clone(), occurrence.scale.clone(), true);
            }
        }
        let name = base.to_owned();
        if self.section_meter(bar_starts[0]) != self.meter_at(start) {
            omissions.push(ScoreExportOmission {
                subject: format!("region \"{base}\""),
                message: "meter change is not aligned to a bar boundary".into(),
            });
        }
        Ok(SongSection {
            base: base.to_owned(),
            name,
            start_tick: start,
            end_tick: end,
            bar_lengths,
            bar_starts,
            meter: self.section_meter(start),
            bpm: self.bpm_at(start),
            tonic: tonics.0,
            scale: tonics.1,
            inherited: tonics.2,
        })
    }

    fn section_meter(&self, tick: u64) -> (u8, u8) {
        self.meter_at(tick)
    }
}

fn defaults() -> (String, Option<String>, bool) {
    ("C".into(), None, false)
}

/// Resolves every Track that becomes a Score instrument or kit.
fn resolve_tracks<'a>(
    session: &'a CreativeSession,
    omissions: &mut Vec<ScoreExportOmission>,
) -> Vec<ExportTrack<'a>> {
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
            TrackKind::Instrument => match track.score_instrument.as_deref() {
                Some(score) if score.starts_with("kit:") => TrackRole::Kit(
                    identifier(&score[4..]).unwrap_or_else(|| format!("kit_{}", index + 1)),
                ),
                _ if track.instrument.is_some() => TrackRole::Instrument,
                _ => {
                    if has_notes(session, &track.id) {
                        omissions.push(ScoreExportOmission {
                            subject: format!("track \"{}\"", track.name),
                            message: "the track has notes but no instrument".into(),
                        });
                    }
                    continue;
                }
            },
        };
        let base = match &role {
            TrackRole::Kit(kit) => kit.clone(),
            _ => base,
        };
        let id = unique_id(&base, &format!("track_{}", index + 1), &mut used);
        tracks.push(ExportTrack { id, track, role });
    }
    tracks
}

fn has_notes(session: &CreativeSession, track_id: &str) -> bool {
    session
        .arrangement
        .midi_clips
        .iter()
        .any(|clip| clip.track_id == track_id && !clip.notes.is_empty())
}

/// Resolves kit member identifiers from the kit's trigger notes.
fn kit_members(export: &Export) -> BTreeMap<String, Vec<KitMember>> {
    let mut kits = BTreeMap::new();
    for track in &export.tracks {
        let TrackRole::Kit(kit) = &track.role else {
            continue;
        };
        let mut triggers = BTreeSet::new();
        for clip in clips_for(export.session, &track.track.id) {
            for note in &clip.notes {
                triggers.insert(note.note);
            }
        }
        let mut parts = Vec::new();
        for trigger in triggers {
            let id = export
                .original
                .as_ref()
                .and_then(|original| original_kit_part(original, kit, trigger))
                .unwrap_or_else(|| format!("{kit}_{trigger}"));
            parts.push(KitMember { id, trigger });
        }
        kits.insert(kit.clone(), parts);
    }
    kits
}

fn original_kit_part(original: &Original, kit: &str, trigger: u8) -> Option<String> {
    let instruments = original.field(&["instruments"])?.as_object()?;
    instruments.iter().find_map(|(id, spec)| {
        (spec.get("kit").and_then(Value::as_str) == Some(kit)
            && spec
                .get("trigger_note_midi")
                .and_then(Value::as_u64)
                .and_then(|note| u8::try_from(note).ok())
                == Some(trigger))
        .then(|| identifier(id))
        .flatten()
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
fn stage(path: &Path, files: &BTreeMap<String, Vec<u8>>, score: &str) -> Result<(), ExportError> {
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
        for (relative, bytes) in files {
            let target = staging.join(relative);
            if let Some(parent) = target.parent() {
                std::fs::create_dir_all(parent).map_err(|error| {
                    ExportError::new(format!("could not create {}: {error}", parent.display()))
                })?;
            }
            std::fs::write(&target, bytes).map_err(|error| {
                ExportError::new(format!("could not write {}: {error}", target.display()))
            })?;
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

/// Copies a package directory into the staged song folder.
pub(super) fn copy_package(
    export: &mut Export,
    source: &Path,
    destination: &str,
) -> Result<(), ExportError> {
    copy_tree(source, source, destination, &mut export.files)
}

fn copy_tree(
    root: &Path,
    directory: &Path,
    destination: &str,
    files: &mut BTreeMap<String, Vec<u8>>,
) -> Result<(), ExportError> {
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
            copy_tree(root, &path, &relative, files)?;
        } else {
            let bytes = std::fs::read(&path).map_err(|error| {
                ExportError::new(format!("could not read {}: {error}", path.display()))
            })?;
            files.insert(relative, bytes);
        }
    }
    let _ = root;
    Ok(())
}

/// Adds a relative file to the staged folder.
pub(super) fn add_file(
    export: &mut Export,
    relative: String,
    bytes: Vec<u8>,
) -> Result<(), ExportError> {
    if export.files.insert(relative.clone(), bytes).is_some() {
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
    use riffra_core::{CreativeSession, InstrumentControlEventKind, TimelineTick};

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
            "verse_line":["1:8 3:8","5:8 1':8"],
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

    fn root(name: &str) -> PathBuf {
        let root = std::env::temp_dir().join(format!(
            "riffra-score-export-{name}-{}",
            riffra_control::new_instance_id()
        ));
        std::fs::create_dir_all(&root).unwrap();
        root
    }

    fn catalog(root: &Path) {
        let entries = [
            ("Synth", PluginRole::Instrument),
            ("Bass", PluginRole::Instrument),
            ("Drums", PluginRole::Instrument),
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
    fn pan_laws_convert_to_matching_linear_balance_gains() {
        let (gain, pan) = pan_law(PanLaw::EqualPower, 0.0, 0.0);
        assert!((gain - 20.0 * (std::f64::consts::FRAC_1_SQRT_2).log10()).abs() < 1e-9);
        assert!(pan.abs() < 1e-9);

        let (gain, pan) = pan_law(PanLaw::EqualPower, -6.0, -1.0);
        assert!((gain + 6.0).abs() < 1e-9);
        assert!((pan + 1.0).abs() < 1e-9);

        let (gain, pan) = pan_law(PanLaw::UnityCenterStereo, 3.0, 1.0);
        assert!((gain - 3.0).abs() < 1e-9);
        assert!((pan - 1.0).abs() < 1e-9);

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
    }

    /// Collects the musical content the round trip must preserve.
    fn performance(session: &CreativeSession) -> String {
        let mut lines = Vec::new();
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
                    notes.push(format!(
                        "{}:{}:{}:{}",
                        clip.start_tick.0 + note.start_tick.0,
                        note.duration_ticks,
                        note.note,
                        note.velocity
                    ));
                }
                for event in &clip.instrument_control_events {
                    let tick = clip.start_tick.0 + event.tick.0;
                    controls.push(match &event.kind {
                        InstrumentControlEventKind::SustainPedal { down } => {
                            format!("{tick}:sustain:{down}")
                        }
                        InstrumentControlEventKind::PitchBend { value } => {
                            format!("{tick}:bend:{value}")
                        }
                        InstrumentControlEventKind::ModWheel { value } => {
                            format!("{tick}:mod:{value}")
                        }
                        InstrumentControlEventKind::Aftertouch { value } => {
                            format!("{tick}:after:{value}")
                        }
                        InstrumentControlEventKind::ParameterChange {
                            parameter,
                            native_value,
                        } => format!("{tick}:parameter:{parameter}:{native_value}"),
                        InstrumentControlEventKind::ParameterRamp {
                            parameter,
                            from_value,
                            to_value,
                            duration_ticks,
                        } => format!(
                            "{tick}:ramp:{parameter}:{from_value}:{to_value}:{duration_ticks}"
                        ),
                        InstrumentControlEventKind::KeySwitch { note, mode, down } => {
                            format!("{tick}:keyswitch:{note}:{mode:?}:{down}")
                        }
                        InstrumentControlEventKind::ControlChange { number, value } => {
                            format!("{tick}:cc:{number}:{value}")
                        }
                    });
                }
            }
            notes.sort();
            controls.sort();
            lines.push(format!("{id}|{}|{}", notes.join(","), controls.join(",")));
        }
        lines.sort();
        for change in &session.arrangement.timebase.tempo_changes {
            lines.push(format!("tempo:{}:{}", change.tick, change.bpm));
        }
        for change in &session.arrangement.timebase.time_signature_changes {
            lines.push(format!(
                "meter:{}:{}/{}",
                change.tick, change.numerator, change.denominator
            ));
        }
        for bus in &session.arrangement.mixer.buses {
            lines.push(format!("bus:{}:{:?}", bus.name, bus.kind));
        }
        lines.join("\n")
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
        "instruments":{"part":{"source":{
            "vst3":{"plugin":"Synth"},
            "articulations":{
                "default":{"keyswitch":24,"mode":"latch"},
                "mute":{"keyswitch":26,"mode":"latch"},
                "open":{"keyswitch":27,"mode":"hold"}
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
        let session = crate::score_yaml::import::session_from_score(
            ARTICULATION_SCORE,
            &root.join("source"),
            &root,
            catalog,
        );
        let first = exported(&session, &root, catalog);
        let text = std::fs::read_to_string(first.path.join("score.yaml")).unwrap();
        crate::score::compile(&text).expect("the exported Score compiles");
        let imported = crate::score_yaml::import::session_from_score(
            &text,
            &root.join("song"),
            &root,
            catalog,
        );
        assert_eq!(key_switches(&session), key_switches(&imported));
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
