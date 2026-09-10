import type {
  AudioClip,
  CreativeSession,
  Marker,
  MidiClip,
  ProjectTimebase,
  TrackKind,
} from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import type { ContextMenuItem } from '@/shared/ui/ContextMenu';
import { toast } from '@/shared/toasts';
import {
  clipDurationTicks,
  countOffGridNotes,
  snapGridTicks,
  type SnapGrid,
} from '../model/arrange-timeline';
import { useState } from 'react';
import type { useArrangeDetailController } from './useArrangeDetailController';
import type { useArrangeEditor } from './useArrangeEditor';
import type { useArrangeRulerController } from './useArrangeRulerController';

export interface ArrangePluginPickerRequest {
  trackId: string;
  kind: 'effect' | 'instrument';
}

interface UseArrangeContextMenusOptions {
  arrangement: CreativeSession['arrangement'];
  api: Pick<
    ArrangeApi,
    | 'duplicateAudioClip'
    | 'updateAudioClip'
    | 'removeTimelineClips'
    | 'duplicateMidiClip'
    | 'updateMidiClip'
    | 'quantizeMidiNotes'
  >;
  editor: ReturnType<typeof useArrangeEditor>;
  ruler: ReturnType<typeof useArrangeRulerController>;
  detail: ReturnType<typeof useArrangeDetailController>;
  snap: SnapGrid;
  timebase: ProjectTimebase;
  displayTick: number;
  setPluginPicker: (picker: ArrangePluginPickerRequest | null) => void;
  addTrack: (kind: TrackKind) => Promise<CreativeSession | null>;
  deleteTrack: (trackId: string, name: string, clipCount: number) => void;
  trackClipCounts: Map<string, number>;
  createEmptyMidiClip: (trackId: string, rawTick: number) => Promise<void>;
}

export function useArrangeContextMenus({
  arrangement,
  api,
  editor,
  ruler,
  detail,
  snap,
  timebase,
  displayTick,
  setPluginPicker,
  addTrack,
  deleteTrack,
  trackClipCounts,
  createEmptyMidiClip,
}: UseArrangeContextMenusOptions) {
  const [contextMenu, setContextMenu] = useState<{
    x: number;
    y: number;
    items: ContextMenuItem[];
  } | null>(null);
  const closeContextMenu = () => setContextMenu(null);

  const openRulerContextMenu = (event: React.MouseEvent<HTMLDivElement>, tick: number) => {
    event.preventDefault();
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Add Marker Here', onClick: () => ruler.addMarkerAt(tick) },
        {
          label: 'Set Loop to Selection',
          onClick: ruler.setLoopToSelection,
          disabled: !ruler.timeSelection,
        },
        {
          label: 'Set Punch Range',
          onClick: ruler.setPunchToSelection,
          disabled: !ruler.timeSelection,
        },
        { separator: true },
        {
          label: 'Clear Loop',
          onClick: () => ruler.clearRange('loop'),
          disabled: !arrangement.loopRange.enabled,
        },
        {
          label: 'Clear Punch',
          onClick: () => ruler.clearRange('punch'),
          disabled: !arrangement.punchRange,
        },
      ],
    });
  };

  const openRangeContextMenu = (
    event: React.MouseEvent<HTMLDivElement>,
    range: 'loop' | 'punch',
  ) => {
    event.preventDefault();
    ruler.selectRange(range);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            ruler.clearRange(range);
            ruler.clearSelectedRange();
          },
        },
      ],
    });
  };

  const openMarkerContextMenu = (event: React.MouseEvent, marker: Marker) => {
    event.preventDefault();
    ruler.selectMarker(marker.id);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Rename', onClick: () => ruler.renameMarker(marker) },
        { label: 'Delete', danger: true, onClick: () => ruler.removeMarker(marker) },
      ],
    });
  };

  const openAudioClipContextMenu = (event: React.MouseEvent, clip: AudioClip) => {
    event.preventDefault();
    event.stopPropagation();
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'Split at Playhead',
          onClick: () => {
            closeContextMenu();
            void editor.splitClip(clip, displayTick);
          },
        },
        {
          label: 'Duplicate',
          onClick: () => {
            closeContextMenu();
            void editor.commit(api.duplicateAudioClip(clip.id));
          },
        },
        {
          label: clip.muted ? 'Unmute' : 'Mute',
          onClick: () => {
            closeContextMenu();
            void editor.commit(api.updateAudioClip(clip.id, { muted: !clip.muted }));
          },
        },
        {
          label: clip.loopEnabled ? 'Disable Loop' : 'Enable Loop',
          onClick: () => {
            closeContextMenu();
            void editor.commit(api.updateAudioClip(clip.id, { loopEnabled: !clip.loopEnabled }));
          },
        },
        {
          label: 'Merge with Previous',
          disabled: !arrangement.audioClips.some(
            (item) =>
              item.id !== clip.id &&
              item.trackId === clip.trackId &&
              item.assetId === clip.assetId &&
              item.startTick + clipDurationTicks(item, timebase) === clip.startTick &&
              item.sourceRange.end === clip.sourceRange.start,
          ),
          onClick: () => {
            closeContextMenu();
            void editor.mergeAudioClipWithPrevious(clip);
          },
        },
        {
          label: 'Merge with Next',
          disabled: !arrangement.audioClips.some(
            (item) =>
              item.id !== clip.id &&
              item.trackId === clip.trackId &&
              item.assetId === clip.assetId &&
              item.startTick === clip.startTick + clipDurationTicks(clip, timebase) &&
              item.sourceRange.start === clip.sourceRange.end,
          ),
          onClick: () => {
            closeContextMenu();
            void editor.mergeAudioClipWithNext(clip);
          },
        },
        { separator: true },
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            closeContextMenu();
            void editor.commit(api.removeTimelineClips([clip.id], []));
          },
        },
      ],
    });
  };

  const openMidiClipContextMenu = (event: React.MouseEvent, clip: MidiClip) => {
    event.preventDefault();
    event.stopPropagation();
    const gridTicks = snapGridTicks(snap, timebase);
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        {
          label: 'Open MIDI Editor',
          onClick: () => {
            closeContextMenu();
            detail.openMidiEditor(clip);
          },
        },
        {
          label: 'Split at Playhead',
          onClick: () => {
            closeContextMenu();
            void editor.splitMidiClip(clip, displayTick);
          },
        },
        {
          label: 'Duplicate',
          onClick: () => {
            closeContextMenu();
            void editor.commit(api.duplicateMidiClip(clip.id));
          },
        },
        {
          label: clip.muted ? 'Unmute' : 'Mute',
          onClick: () => {
            closeContextMenu();
            void editor.commit(api.updateMidiClip(clip.id, { muted: !clip.muted }));
          },
        },
        {
          label: clip.loopEnabled ? 'Disable Loop' : 'Enable Loop',
          onClick: () => {
            closeContextMenu();
            void editor.commit(api.updateMidiClip(clip.id, { loopEnabled: !clip.loopEnabled }));
          },
        },
        {
          label: 'Quantize',
          disabled: gridTicks === 0,
          onClick: () => {
            closeContextMenu();
            const offGrid = countOffGridNotes(clip.notes, gridTicks);
            if (offGrid === 0) {
              toast('Notes are already on the grid.');
              return;
            }
            void editor
              .commit(
                api.quantizeMidiNotes(
                  clip.id,
                  clip.notes.map((note) => note.id),
                  gridTicks,
                ),
              )
              .then((next) => {
                if (next)
                  toast(`Quantized ${offGrid} note${offGrid === 1 ? '' : 's'} to the grid.`);
              });
          },
        },
        {
          label: 'Merge with Previous',
          disabled: !arrangement.midiClips.some(
            (item) =>
              item.id !== clip.id &&
              item.trackId === clip.trackId &&
              item.startTick + item.durationTicks === clip.startTick,
          ),
          onClick: () => {
            closeContextMenu();
            void editor.mergeMidiClipWithPrevious(clip);
          },
        },
        {
          label: 'Merge with Next',
          disabled: !arrangement.midiClips.some(
            (item) =>
              item.id !== clip.id &&
              item.trackId === clip.trackId &&
              item.startTick === clip.startTick + clip.durationTicks,
          ),
          onClick: () => {
            closeContextMenu();
            void editor.mergeMidiClipWithNext(clip);
          },
        },
        { separator: true },
        {
          label: 'Delete',
          danger: true,
          onClick: () => {
            closeContextMenu();
            void editor.commit(api.removeTimelineClips([], [clip.id]));
          },
        },
      ],
    });
  };

  const openTrackAreaContextMenu = (event: React.MouseEvent, trackId: string | null, tick = 0) => {
    event.preventDefault();
    const track = trackId ? arrangement.tracks.find((item) => item.id === trackId) : undefined;
    if (trackId && !track) return;
    setContextMenu({
      x: event.clientX,
      y: event.clientY,
      items: [
        { label: 'Add Audio Track', onClick: () => void addTrack('audio') },
        { label: 'Add Instrument Track', onClick: () => void addTrack('instrument') },
        ...(track
          ? [
              { separator: true },
              ...(track.kind === 'instrument'
                ? [
                    {
                      label: 'Insert MIDI Clip',
                      onClick: () => {
                        closeContextMenu();
                        void createEmptyMidiClip(track.id, tick);
                      },
                    },
                    { separator: true },
                  ]
                : []),
              {
                label: track.kind === 'audio' ? 'Add Effect' : 'Choose Instrument',
                onClick: () =>
                  setPluginPicker({
                    trackId: track.id,
                    kind: track.kind === 'audio' ? 'effect' : 'instrument',
                  }),
              },
              { separator: true },
              {
                label: 'Delete Track',
                danger: true,
                onClick: () =>
                  void deleteTrack(track.id, track.name, trackClipCounts.get(track.id) ?? 0),
              },
            ]
          : []),
      ],
    });
  };

  return {
    contextMenu,
    closeContextMenu,
    openRulerContextMenu,
    openRangeContextMenu,
    openMarkerContextMenu,
    openAudioClipContextMenu,
    openMidiClipContextMenu,
    openTrackAreaContextMenu,
  };
}
