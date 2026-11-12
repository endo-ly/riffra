import { useSessionCommand } from '@/shared/session/useSessionCommand';
import { useMemo, useState } from 'react';
import type { TrackKind } from '@/model/generated';
import { ticksPerBar } from '@/model/timebase';
import { useSession, useSessionContext, useSessionStore } from '@/shared/session/session-context';
import { errorText } from '@/shared/errors';
import { HostConnectionChangedError } from '@/native/invoke';
import { showToast } from '@/shared/toasts';
import { reorderTargetIndex } from '@/features/timeline/model/timeline-layout';

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
  const { api } = useSessionContext();
  const store = useSessionStore();
  const commit = useSessionCommand();
  const arrangement = useSession((state) => state.project.boot!.canonical.session.arrangement);
  const focusedTrackId = useSession((state) => state.workspace.focusedTrackId);
  const openMidiEditor = useSession((state) => state.workspace.openMidiEditor);
  const [confirmRequest, setConfirmRequest] = useState<WorkspaceConfirmRequest | null>(null);
  const clipsByTrack = useSession((state) => state.readModel!.clipsByTrack);
  const clipCounts = useMemo(
    () =>
      new Map([...clipsByTrack].map(([id, clips]) => [id, clips.audio.length + clips.midi.length])),
    [clipsByTrack],
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
    const origin = store.getState().scope;
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
    if (store.getState().scope !== origin) return;
    await commit(api.removeTrack(trackId));
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
