import { useCallback, useState } from 'react';
import type { AudioClip, AudioClipPatch, CreativeSession, SessionOpRunner } from '@/lib/domain';
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
  runSessionOp,
}: {
  session: CreativeSession;
  setSession: (value: CreativeSession) => void;
  api: NativeApi;
  runSessionOp: SessionOpRunner;
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
          runSessionOp={runSessionOp}
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
  runSessionOp,
}: {
  clip: AudioClip;
  session: CreativeSession;
  setSession: (value: CreativeSession) => void;
  api: NativeApi;
  runSessionOp: SessionOpRunner;
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
      const next = await runSessionOp(
        () => api.updateAudioClip(clip.id, { [field]: parsed } as AudioClipPatch),
        'Edit clip',
      );
      if (next) setSession(next);
    },
    [api, clip, drafts, runSessionOp, setSession],
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
    const next = await runSessionOp(() => api.updateAudioClip(clip.id, patch), 'Edit clip');
    if (next) setSession(next);
  }, [api, clip, drafts, runSessionOp, setSession]);

  const changeTrack = useCallback(
    async (trackId: string) => {
      await commitAllDrafts();
      const next = await runSessionOp(
        () => api.moveAudioClipToTrack(clip.id, trackId),
        'Move clip',
      );
      if (next) setSession(next);
    },
    [api, clip.id, commitAllDrafts, runSessionOp, setSession],
  );

  const toggleMute = useCallback(async () => {
    await commitAllDrafts();
    const next = await runSessionOp(
      () => api.setAudioClipMuted(clip.id, !clip.muted),
      'Toggle mute',
    );
    if (next) setSession(next);
  }, [api, clip.id, clip.muted, commitAllDrafts, runSessionOp, setSession]);

  const toggleLoop = useCallback(async () => {
    await commitAllDrafts();
    const next = await runSessionOp(
      () => api.setAudioClipLoop(clip.id, !clip.loopEnabled),
      'Toggle loop',
    );
    if (next) setSession(next);
  }, [api, clip.id, clip.loopEnabled, commitAllDrafts, runSessionOp, setSession]);

  const duplicate = useCallback(async () => {
    await commitAllDrafts();
    const next = await runSessionOp(() => api.duplicateAudioClip(clip.id), 'Duplicate clip');
    if (next) setSession(next);
  }, [api, clip.id, commitAllDrafts, runSessionOp, setSession]);

  const split = useCallback(async () => {
    await commitAllDrafts();
    // Split at the center of the clip's current production state. The offset is
    // computed on the Rust side from the persisted clip duration, so a draft
    // duration edit committed just above is always reflected.
    const next = await runSessionOp(() => api.splitAudioClip(clip.id), 'Split clip');
    if (next) setSession(next);
  }, [api, clip.id, commitAllDrafts, runSessionOp, setSession]);

  const remove = useCallback(async () => {
    await commitAllDrafts();
    const next = await runSessionOp(() => api.removeAudioClip(clip.id), 'Remove clip');
    if (next) setSession(next);
  }, [api, clip.id, commitAllDrafts, runSessionOp, setSession]);

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
