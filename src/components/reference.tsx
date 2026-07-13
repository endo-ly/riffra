import { useEffect, useState } from 'react';
import type { AiChangeSet, AudioAnalysis, RecordingAsset, Session } from '@/lib/domain';
import { compareAnalyses } from '@/lib/domain';

export function ReferenceCompare({
  analysis,
  recordings,
  references,
  referenceId,
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
  const current =
    recordings.find(
      (recording) => (recording.processedPath ?? recording.rawPath) === analysis?.path,
    ) ?? null;
  const comparison =
    analysis && reference ? compareAnalyses(analysis, references[reference.id] ?? analysis) : null;
  return (
    <section className="section-card reference-card">
      <header>
        <div>
          <span className="eyebrow">REFERENCE COMPARE</span>
          <h2>Loudness-matched read-only view</h2>
        </div>
        <div>
          <span className="status-tag">OFFLINE</span>
          {current && (
            <button
              className="text-button"
              onClick={previewingId === current.id ? onStop : () => onPreview(current)}
            >
              {previewingId === current.id ? 'Stop current' : 'Preview current'}
            </button>
          )}
          {current && reference && (
            <button className="text-button" onClick={syncPreviewing ? onStop : onSyncPreview}>
              {syncPreviewing ? 'Stop sync' : 'Sync preview'}
            </button>
          )}
          <label className="reference-loop">
            <input type="checkbox" checked={loopPreview} onChange={onToggleLoop} /> Loop preview
          </label>
        </div>
      </header>
      {!analysis ? (
        <p className="inspector-copy">Analyze a recording first, then choose a reference.</p>
      ) : (
        <>
          <div className="reference-source-list">
            {recordings.length === 0 ? (
              <small className="inspector-copy">No Inbox recordings are available.</small>
            ) : (
              recordings.slice(0, 8).map((recording) => (
                <div
                  className={`reference-source-row ${recording.id === referenceId ? 'active' : ''}`}
                  key={recording.id}
                >
                  <button className="reference-source" onClick={() => onSelect(recording)}>
                    <strong>{recording.name}</strong>
                    <small>
                      {recording.state} · {recording.samplesWritten.toLocaleString()} samples
                    </small>
                  </button>
                  <button
                    className="text-button"
                    onClick={previewingId === recording.id ? onStop : () => onPreview(recording)}
                  >
                    {previewingId === recording.id ? 'Stop' : 'Preview'}
                  </button>
                </div>
              ))
            )}
          </div>
          {comparison && (
            <div className="comparison-grid">
              <div>
                <span className="eyebrow">RMS DELTA</span>
                <strong>
                  {comparison.rmsDeltaDb >= 0 ? '+' : ''}
                  {comparison.rmsDeltaDb.toFixed(1)} dB
                </strong>
              </div>
              <div>
                <span className="eyebrow">PEAK DELTA</span>
                <strong>
                  {comparison.peakDeltaDb >= 0 ? '+' : ''}
                  {comparison.peakDeltaDb.toFixed(1)} dB
                </strong>
              </div>
              <div>
                <span className="eyebrow">MATCH GAIN</span>
                <strong>
                  {comparison.loudnessMatchGainDb >= 0 ? '+' : ''}
                  {comparison.loudnessMatchGainDb.toFixed(1)} dB
                </strong>
              </div>
              <div>
                <span className="eyebrow">DURATION</span>
                <strong>
                  {comparison.durationDeltaMs >= 0 ? '+' : ''}
                  {(comparison.durationDeltaMs / 1000).toFixed(2)} s
                </strong>
              </div>
            </div>
          )}
        </>
      )}
    </section>
  );
}

export function ReferenceSuggestion({
  analysis,
  recordings,
  references,
  referenceId,
  session,
  setSession,
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
  session: Session;
  setSession: (value: Session) => void;
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
  const contextAllowsAnalysis = session.aiContext.includes('analysis');
  const contextAllowsSelectedClip = session.aiContext.includes('selectedClip');
  const comparison =
    contextAllowsAnalysis && contextAllowsSelectedClip && analysis && referenceAnalysis
      ? compareAnalyses(analysis, referenceAnalysis)
      : null;
  const targetClip = analysis
    ? session.timeline.find((clip) => clip.assetPath === analysis.path)
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
  const applySuggestion = () => {
    if (
      !comparison ||
      !targetClip ||
      proposedGain == null ||
      !selected ||
      session.aiPermission !== 'Apply'
    )
      return;
    const changeSet: AiChangeSet = {
      id: `ai:${Date.now()}`,
      createdAtMs: Date.now(),
      permission: session.aiPermission,
      target: targetClip.id,
      currentGainDb: targetClip.gainDb,
      proposedGainDb: proposedGain,
      reason: 'Match the selected reference RMS without changing the source WAV.',
      expectedEffect: 'A closer perceived level while clip position and source remain unchanged.',
      risk: 'Low · reversible',
      context: [...session.aiContext],
      applied: true,
    };
    setSession({
      ...session,
      timeline: session.timeline.map((clip) =>
        clip.id === targetClip.id ? { ...clip, gainDb: proposedGain } : clip,
      ),
      aiHistory: [...session.aiHistory, changeSet].slice(-128),
    });
    setApplied(true);
  };
  const toggleContext = (id: string) =>
    setSession({
      ...session,
      aiContext: session.aiContext.includes(id)
        ? session.aiContext.filter((item) => item !== id)
        : [...session.aiContext, id],
    });
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
              value={session.aiPermission}
              onChange={(event) =>
                setSession({
                  ...session,
                  aiPermission: event.target.value as Session['aiPermission'],
                })
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
                checked={session.aiContext.includes(item.id)}
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
              <span className="eyebrow">AI CHANGESET · {session.aiPermission.toUpperCase()}</span>
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
                  disabled={!selected || applied || session.aiPermission !== 'Apply'}
                  onClick={applySuggestion}
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
              {session.aiPermission !== 'Apply' && (
                <small className="changeset-preview">
                  Applying is locked in {session.aiPermission} mode. Select Apply only after
                  reviewing this ChangeSet.
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
      {session.aiHistory.length > 0 && (
        <section className="section-card ai-history-card">
          <header>
            <div>
              <span className="eyebrow">AI HISTORY</span>
              <h2>Applied ChangeSets</h2>
            </div>
            <small>{session.aiHistory.length} persisted</small>
          </header>
          {session.aiHistory
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
