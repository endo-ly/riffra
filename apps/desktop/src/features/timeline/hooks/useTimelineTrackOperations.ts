import { useMemo, useState } from 'react';
import type { TrackKind } from '@/model/generated';
import { ticksPerBar } from '@/model/timebase';
import { useSession, useSessionContext } from '@/shared/session/session-context';
import { errorText } from '@/shared/errors';
import { HostConnectionChangedError } from '@/native/invoke';
import { showToast } from '@/shared/toasts';
import { reorderTargetIndex, trackClipCounts } from '@/features/timeline/model/timeline-layout';

/** Track edit that needs the user's confirmation before it runs. */
export interface WorkspaceConfirmRequest {
  title: string;
  message: string;
  confirmLabel?: string;
  danger?: boolean;
  onConfirm: () => void;
}

interface TrackOperationOptions {
  snapTick(rawTick: number): number;
  timeSelection: { startTick: number; endTick: number } | null;
  clearTimeSelection(): void;
}

/** Track creation, deletion and reordering for the timeline. */
export function useTimelineTrackOperations(options: TrackOperationOptions) {
  const { api, commit } = useSessionContext();
  const arrangement = useSession((state) => state.project.boot!.canonical.session.arrangement);
  const selection = useSession((state) => state.workspace.selection);
  const focusedTrackId = useSession((state) => state.workspace.focusedTrackId);
  const lowerView = useSession((state) => state.workspace.lower.view);
  const setSelection = useSession((state) => state.workspace.setSelection);
  const focusTrack = useSession((state) => state.workspace.focusTrack);
  const openMidiEditor = useSession((state) => state.workspace.openMidiEditor);
  const closeLower = useSession((state) => state.workspace.closeLower);
  const [confirmRequest, setConfirmRequest] = useState<WorkspaceConfirmRequest | null>(null);
  const { audioClips, midiClips } = arrangement;
  const clipCounts = useMemo(
    () => trackClipCounts({ audioClips, midiClips }),
    [audioClips, midiClips],
  );
  const barTicks = ticksPerBar(arrangement.timebase);

  const addTrack = (kind: TrackKind) =>
    commit(
      api.addTrack(
        `${kind === 'audio' ? 'Audio' : 'Instrument'} ${arrangement.tracks.filter((track) => track.kind === kind).length + 1}`,
        kind,
      ),
    );

  const createEmptyMidiClip = async (trackId: string, rawTick: number) => {
    const track = arrangement.tracks.find((item) => item.id === trackId);
    if (!track || track.kind !== 'instrument') return;
    const startTick = options.timeSelection
      ? options.timeSelection.startTick
      : options.snapTick(rawTick);
    const durationTicks = options.timeSelection
      ? Math.max(1, options.timeSelection.endTick - options.timeSelection.startTick)
      : Math.max(1, barTicks);
    const next = await commit(api.createMidiClip(trackId, startTick, durationTicks));
    if (!next) return;
    const createdId = next.createdEntityIds.midiClips?.[0];
    const created = next.canonical.session.arrangement.midiClips.find(
      (clip) => clip.id === createdId,
    );
    if (!created) return;
    options.clearTimeSelection();
    openMidiEditor(created.id);
  };

  const performDeleteTrack = async (trackId: string) => {
    const deletedTrack = arrangement.tracks.find((track) => track.id === trackId);
    if (
      focusedTrackId === trackId &&
      deletedTrack?.kind === 'instrument' &&
      deletedTrack.instrument
    ) {
      try {
        await api.panicMidiTrack(trackId);
      } catch (error) {
        if (error instanceof HostConnectionChangedError) return;
        showToast('timeline', errorText(error), { kind: 'error' });
      }
    }
    const next = await commit(api.removeTrack(trackId));
    if (!next) return;
    if (selection.kind === 'track' && selection.trackId === trackId && lowerView === 'devices') {
      closeLower();
    }
    if (focusedTrackId === trackId) focusTrack(null);
    const remaining = new Set([
      ...next.canonical.session.arrangement.audioClips.map((clip) => clip.id),
      ...next.canonical.session.arrangement.midiClips.map((clip) => clip.id),
    ]);
    const selectedClipIds = selection.kind === 'clips' ? selection.clipIds : [];
    const clipIds = selectedClipIds.filter((id) => remaining.has(id));
    if (selection.kind !== 'track' || selection.trackId === trackId) {
      setSelection(clipIds.length ? { kind: 'clips', clipIds } : { kind: 'none' });
    }
  };

  const deleteTrack = (trackId: string, name: string, clipCount: number) => {
    const clipDetail = clipCount
      ? ` This also removes ${clipCount} Clip${clipCount === 1 ? '' : 's'} from the Timeline.`
      : '';
    setConfirmRequest({
      title: `Delete ${name}`,
      message: `${clipDetail}\n\nSource assets will be kept.`,
      confirmLabel: 'Delete Track',
      danger: true,
      onConfirm: () => {
        setConfirmRequest(null);
        void performDeleteTrack(trackId);
      },
    });
  };

  const reorderTrack = (sourceTrackId: string, targetIndex: number, insertAfter: boolean) => {
    const sourceIndex = arrangement.tracks.findIndex((track) => track.id === sourceTrackId);
    if (sourceIndex < 0 || sourceIndex === targetIndex) return;
    void commit(
      api.reorderTrack(sourceTrackId, reorderTargetIndex(sourceIndex, targetIndex, insertAfter)),
    );
  };

  return {
    clipCounts,
    addTrack,
    createEmptyMidiClip,
    deleteTrack,
    reorderTrack,
    confirmRequest,
    dismissConfirm: () => setConfirmRequest(null),
  };
}
