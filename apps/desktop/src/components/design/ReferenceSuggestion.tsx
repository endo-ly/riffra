import { useEffect, useState } from 'react';
import type { AudioAnalysis, CreativeSession, RecordingAsset } from '@/lib/domain';
import { compareAnalyses } from '@/lib/domain';
import type { NativeApi } from '@/native/native-api';
import { ReferenceCompare } from './ReferenceCompare';

export function ReferenceSuggestion({
  analysis,
  recordings,
  references,
  referenceId,
  session,
  setSession,
  api,
  onSelect,
  onPreview,
  onStop,
  onSyncPreview,
  onToggleLoop,
  previewingId,
  syncPreviewing,
  loopPreview,
}: {
  analysis: AudioAnalysis | null;
  recordings: RecordingAsset[];
  references: Record<string, AudioAnalysis>;
  referenceId: string | null;
  session: CreativeSession;
  setSession: (value: CreativeSession) => void;
  api: NativeApi;
  onSelect: (recording: RecordingAsset) => void;
  onPreview: (recording: RecordingAsset) => void;
  onStop: () => void;
  onSyncPreview: () => void;
  onToggleLoop: () => void;
  previewingId: string | null;
  syncPreviewing: boolean;
  loopPreview: boolean;
}) {
  const reference = recordings.find((recording) => recording.id === referenceId) ?? null;
  const referenceAnalysis = reference ? references[reference.id] : null;
  const contextAllowsAnalysis = session.settings.aiContext.includes('analysis');
  const contextAllowsSelectedClip = session.settings.aiContext.includes('selectedClip');
  const comparison =
    contextAllowsAnalysis && contextAllowsSelectedClip && analysis && referenceAnalysis
      ? compareAnalyses(analysis, referenceAnalysis)
      : null;
  const targetClip = analysis
    ? (session.arrangement.audioClips.find(
        (clip) => clip.assetId === session.designContext.targetAssetId,
      ) ?? session.arrangement.audioClips[0])
    : null;
  const [selected, setSelected] = useState(true);
  const [previewing, setPreviewing] = useState(false);
  const [applied, setApplied] = useState(false);
  const contextOptions = [
    { id: 'selectedRack', label: 'Selected rack' },
    { id: 'parameterList', label: 'Parameter list' },
    { id: 'analysis', label: 'Analysis result' },
    { id: 'selectedClip', label: 'Selected clip' },
    { id: 'project', label: 'Project structure' },
    { id: 'userNote', label: 'User note' },
    { id: 'snapshot', label: 'Snapshot' },
    { id: 'previewAudio', label: 'Preview audio' },
    { id: 'errorLog', label: 'Error log' },
  ];
  const changeKey = `${referenceId ?? 'none'}:${targetClip?.id ?? 'none'}:${comparison?.loudnessMatchGainDb ?? 'none'}`;
  useEffect(() => {
    setSelected(true);
    setPreviewing(false);
    setApplied(false);
  }, [changeKey]);
  const proposedGain =
    targetClip && comparison
      ? Math.max(-90, Math.min(24, targetClip.gainDb + comparison.loudnessMatchGainDb))
      : null;
  const applySuggestion = async () => {
    if (
      !comparison ||
      !targetClip ||
      proposedGain == null ||
      !selected ||
      session.settings.aiPermission !== 'Apply'
    )
      return;
    setSession(await api.applyAiSuggestion(targetClip.id, proposedGain));
    setApplied(true);
  };
  const toggleContext = (id: string) => {
    const aiContext = session.settings.aiContext.includes(id)
      ? session.settings.aiContext.filter((item) => item !== id)
      : [...session.settings.aiContext, id];
    void api.updateSessionSettings({ aiContext }).then(setSession);
  };
  return (
    <>
      <section className="section-card ai-context-card">
        <header>
          <div>
            <span className="eyebrow">AI CONTEXT CONTROL</span>
            <h2>Reversible collaborator</h2>
          </div>
          <label className="ai-permission">
            <span>Permission</span>
            <select
              value={session.settings.aiPermission}
              onChange={(event) =>
                void api
                  .updateSessionSettings({ aiPermission: event.target.value })
                  .then(setSession)
              }
            >
              <option>Explain</option>
              <option>Suggest</option>
              <option>Apply</option>
            </select>
          </label>
        </header>
        <p className="inspector-copy">
          Only the checked local context is eligible for this offline suggestion. Nothing is sent to
          an external provider.
        </p>
        <div className="ai-context-list">
          {contextOptions.map((item) => (
            <label key={item.id}>
              <input
                type="checkbox"
                checked={session.settings.aiContext.includes(item.id)}
                onChange={() => toggleContext(item.id)}
              />
              <span>{item.label}</span>
            </label>
          ))}
        </div>
      </section>
      <ReferenceCompare
        analysis={analysis}
        recordings={recordings}
        references={references}
        referenceId={referenceId}
        targetAssetId={session.designContext.targetAssetId ?? null}
        onSelect={onSelect}
        onPreview={onPreview}
        onStop={onStop}
        onSyncPreview={onSyncPreview}
        onToggleLoop={onToggleLoop}
        previewingId={previewingId}
        syncPreviewing={syncPreviewing}
        loopPreview={loopPreview}
      />
      {(!contextAllowsAnalysis || !contextAllowsSelectedClip) && (
        <section className="section-card ai-context-warning">
          <p className="inspector-copy">
            The suggestion is paused until both Analysis result and Selected clip are included in
            the local context.
          </p>
        </section>
      )}
      {comparison && (
        <section className="section-card suggestion-card">
          <header>
            <div>
              <span className="eyebrow">
                AI CHANGESET · {session.settings.aiPermission.toUpperCase()}
              </span>
              <h2>
                {targetClip
                  ? `Loudness match for ${targetClip.name}`
                  : 'Place a clip before applying'}
              </h2>
            </div>
            <span className={`status-tag ${applied ? 'success' : ''}`}>
              {applied ? 'APPLIED' : 'PREVIEW'}
            </span>
          </header>
          {targetClip && proposedGain != null ? (
            <>
              <div className="changeset-grid">
                <div>
                  <span className="eyebrow">TARGET</span>
                  <strong>{targetClip.name} · Gain dB</strong>
                </div>
                <div>
                  <span className="eyebrow">CURRENT</span>
                  <strong>{targetClip.gainDb.toFixed(1)} dB</strong>
                </div>
                <div>
                  <span className="eyebrow">PROPOSED</span>
                  <strong>{proposedGain.toFixed(1)} dB</strong>
                </div>
                <div>
                  <span className="eyebrow">RISK</span>
                  <strong>Low · reversible</strong>
                </div>
              </div>
              <p className="inspector-copy">
                Reason: match the selected reference RMS without changing the source WAV. Expected
                audible effect: a closer perceived level while clip position and source remain
                unchanged.
              </p>
              <div className="changeset-actions">
                <label>
                  <input
                    type="checkbox"
                    checked={selected}
                    onChange={(event) => setSelected(event.target.checked)}
                  />{' '}
                  Apply selected change
                </label>
                <button className="text-button" onClick={() => setPreviewing((value) => !value)}>
                  {previewing ? 'Previewing' : 'Preview'}
                </button>
                <button
                  className="text-button"
                  disabled={!selected || applied || session.settings.aiPermission !== 'Apply'}
                  onClick={() => void applySuggestion()}
                >
                  Apply selected
                </button>
                <button
                  className="text-button danger"
                  disabled={applied}
                  onClick={() => {
                    setSelected(false);
                    setPreviewing(false);
                  }}
                >
                  Reject
                </button>
              </div>
              {session.settings.aiPermission !== 'Apply' && (
                <small className="changeset-preview">
                  Applying is locked in {session.settings.aiPermission} mode. Select Apply only
                  after reviewing this ChangeSet.
                </small>
              )}
              {previewing && (
                <small className="changeset-preview">
                  Preview only: {comparison.loudnessMatchGainDb >= 0 ? '+' : ''}
                  {comparison.loudnessMatchGainDb.toFixed(1)} dB would be applied. No session state
                  changed.
                </small>
              )}
            </>
          ) : (
            <p className="inspector-copy">
              Place this recording on Arrange to create an explicit, reversible change target.
            </p>
          )}
        </section>
      )}
      {session.settings.aiHistory.length > 0 && (
        <section className="section-card ai-history-card">
          <header>
            <div>
              <span className="eyebrow">AI HISTORY</span>
              <h2>Applied ChangeSets</h2>
            </div>
            <small>{session.settings.aiHistory.length} persisted</small>
          </header>
          {session.settings.aiHistory
            .slice(-8)
            .reverse()
            .map((item) => (
              <article className="ai-history-row" key={item.id}>
                <div>
                  <strong>{item.target}</strong>
                  <small>
                    {new Date(item.createdAtMs).toLocaleString('ja-JP')} · {item.permission} ·{' '}
                    {item.context.join(', ') || 'no context'}
                  </small>
                </div>
                <span className="status-tag success">{item.applied ? 'APPLIED' : 'PREVIEW'}</span>
                <p>
                  {item.currentGainDb.toFixed(1)} dB → {item.proposedGainDb.toFixed(1)} dB ·{' '}
                  {item.reason}
                </p>
              </article>
            ))}
        </section>
      )}
    </>
  );
}
