import { useCallback, useState } from 'react';
import type { AudioClip, AudioClipPatch, CreativeSession } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';

type NumericField =
  | 'positionMs'
  | 'durationMs'
  | 'sourceStartMs'
  | 'sourceEndMs'
  | 'gainDb'
  | 'fadeInMs'
  | 'fadeOutMs'
  | 'pan';

const NUMERIC_FIELDS: readonly NumericField[] = [
  'positionMs',
  'durationMs',
  'sourceStartMs',
  'sourceEndMs',
  'gainDb',
  'fadeInMs',
  'fadeOutMs',
  'pan',
];

const FIELD_LABELS: Record<NumericField, string> = {
  positionMs: 'Start ms',
  durationMs: 'Length ms',
  sourceStartMs: 'Source in',
  sourceEndMs: 'Source out',
  gainDb: 'Gain dB',
  fadeInMs: 'Fade in',
  fadeOutMs: 'Fade out',
  pan: 'Pan',
};

const FIELD_STEP: Partial<Record<NumericField, string>> = {
  gainDb: '0.5',
  pan: '0.05',
};

const FIELD_MIN: Partial<Record<NumericField, string>> = {
  positionMs: '0',
  durationMs: '1',
  sourceStartMs: '0',
  sourceEndMs: '0',
  gainDb: '-90',
  fadeInMs: '0',
  fadeOutMs: '0',
  pan: '-1',
};

const FIELD_MAX: Partial<Record<NumericField, string>> = {
  gainDb: '24',
  pan: '1',
};

export function TimelineClipInspector({
  session,
  setSession,
  api,
}: {
  session: CreativeSession;
  setSession: (value: CreativeSession) => void;
  api: NativeApi;
}) {
  if (!session.arrangement.audioClips.length) return null;
  return (
    <section className="section-card timeline-editor">
      <header>
        <div>
          <span className="eyebrow">CLIP INSPECTOR</span>
          <h2>Non-destructive edits</h2>
        </div>
        <small>Source WAVs remain unchanged</small>
      </header>
      {session.arrangement.audioClips.map((clip) => (
        <ClipEditRow
          key={clip.id}
          clip={clip}
          session={session}
          setSession={setSession}
          api={api}
        />
      ))}
    </section>
  );
}

type DraftMap = Partial<Record<NumericField, string>>;

function ClipEditRow({
  clip,
  session,
  setSession,
  api,
}: {
  clip: AudioClip;
  session: CreativeSession;
  setSession: (value: CreativeSession) => void;
  api: NativeApi;
}) {
  const [drafts, setDrafts] = useState<DraftMap>({});

  const displayedValue = (field: NumericField) =>
    field in drafts ? String(drafts[field]) : String(clip[field]);

  const stageDraft = (field: NumericField, raw: string) =>
    setDrafts((current) => ({ ...current, [field]: raw }));

  const commitField = useCallback(
    async (field: NumericField) => {
      const raw = drafts[field];
      if (raw === undefined) return;
      const parsed = Number(raw);
      setDrafts((current) => {
        if (!(field in current)) return current;
        const next = { ...current };
        delete next[field];
        return next;
      });
      if (!Number.isFinite(parsed)) return;
      if (parsed === clip[field]) return;
      const next = await api.updateAudioClip(clip.id, { [field]: parsed } as AudioClipPatch);
      if (next) setSession(next);
    },
    [api, clip, drafts, setSession],
  );

  const commitAllDrafts = useCallback(async (): Promise<void> => {
    const patch: AudioClipPatch = {};
    let hasPatch = false;
    for (const field of NUMERIC_FIELDS) {
      const raw = drafts[field];
      if (raw === undefined) continue;
      const parsed = Number(raw);
      if (Number.isFinite(parsed) && parsed !== clip[field]) {
        patch[field] = parsed;
        hasPatch = true;
      }
    }
    if (!hasPatch) {
      setDrafts({});
      return;
    }
    setDrafts({});
    const next = await api.updateAudioClip(clip.id, patch);
    if (next) setSession(next);
  }, [api, clip, drafts, setSession]);

  const changeTrack = useCallback(
    async (trackId: string) => {
      await commitAllDrafts();
      const next = await api.moveAudioClipToTrack(clip.id, trackId);
      if (next) setSession(next);
    },
    [api, clip.id, commitAllDrafts, setSession],
  );

  const toggleMute = useCallback(async () => {
    await commitAllDrafts();
    const next = await api.setAudioClipMuted(clip.id, !clip.muted);
    if (next) setSession(next);
  }, [api, clip.id, clip.muted, commitAllDrafts, setSession]);

  const toggleLoop = useCallback(async () => {
    await commitAllDrafts();
    const next = await api.setAudioClipLoop(clip.id, !clip.loopEnabled);
    if (next) setSession(next);
  }, [api, clip.id, clip.loopEnabled, commitAllDrafts, setSession]);

  const duplicate = useCallback(async () => {
    await commitAllDrafts();
    const next = await api.duplicateAudioClip(clip.id);
    if (next) setSession(next);
  }, [api, clip.id, commitAllDrafts, setSession]);

  const split = useCallback(async () => {
    await commitAllDrafts();
    const next = await api.splitAudioClip(clip.id, Math.floor(clip.durationMs / 2));
    if (next) setSession(next);
  }, [api, clip.id, clip.durationMs, commitAllDrafts, setSession]);

  const remove = useCallback(async () => {
    await commitAllDrafts();
    const next = await api.removeAudioClip(clip.id);
    if (next) setSession(next);
  }, [api, clip.id, commitAllDrafts, setSession]);

  return (
    <div className={`timeline-edit-row timeline-edit-row-expanded ${clip.muted ? 'muted' : ''}`}>
      <div className="timeline-edit-name">
        <strong>{clip.name}</strong>
        <small>{clip.assetId}</small>
      </div>
      <label>
        <span>Track</span>
        <select value={clip.trackId} onChange={(event) => void changeTrack(event.target.value)}>
          {session.arrangement.tracks.map((track) => (
            <option value={track.id} key={track.id}>
              {track.name}
            </option>
          ))}
        </select>
      </label>
      {NUMERIC_FIELDS.map((field) => (
        <label key={field}>
          <span>{FIELD_LABELS[field]}</span>
          <input
            type="number"
            min={FIELD_MIN[field]}
            max={FIELD_MAX[field]}
            step={FIELD_STEP[field]}
            value={displayedValue(field)}
            onChange={(event) => stageDraft(field, event.target.value)}
            onBlur={() => void commitField(field)}
            onKeyDown={(event) => {
              if (event.key === 'Enter') {
                event.preventDefault();
                (event.target as HTMLInputElement).blur();
              }
            }}
          />
        </label>
      ))}
      <button className="text-button" onClick={() => void toggleLoop()}>
        {clip.loopEnabled ? 'Loop on' : 'Loop'}
      </button>
      <button className="text-button" onClick={() => void duplicate()}>
        Duplicate
      </button>
      <button className="text-button" onClick={() => void split()}>
        Split
      </button>
      <button className="text-button" onClick={() => void toggleMute()}>
        {clip.muted ? 'Unmute' : 'Mute'}
      </button>
      <button className="text-button danger" onClick={() => void remove()}>
        Remove
      </button>
    </div>
  );
}
