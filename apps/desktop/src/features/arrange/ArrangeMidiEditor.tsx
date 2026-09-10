import type { MutableRefObject, ReactNode } from 'react';
import type {
  ArrangementMutationResult,
  CreativeSession,
  MidiClip,
  ProjectTimebase,
} from '@/model/domain';
import type { ArrangeApi } from '@/native/native-api';
import { MidiEditorPanel, type MidiGhostNote } from './midi-editor/MidiEditorPanel';

type ArrangeMidiApi = Pick<
  ArrangeApi,
  | 'addMidiNote'
  | 'updateMidiNote'
  | 'updateMidiNotes'
  | 'removeMidiNotes'
  | 'insertMidiNotes'
  | 'quantizeMidiNotes'
  | 'duplicateMidiNotes'
>;

interface ArrangeMidiEditorProps {
  clip: MidiClip | null;
  timebase: ProjectTimebase;
  ghostNotes: MidiGhostNote[];
  playheadTick: number;
  playheadTickRef: MutableRefObject<number>;
  playing: boolean;
  onSeek: (tick: number) => void;
  previewAvailable: boolean;
  onSendMidi: (trackId: string, bytes: number[]) => Promise<unknown>;
  onPanicMidi: (trackId: string) => Promise<unknown>;
  toolbarTrailing?: ReactNode;
  api: ArrangeMidiApi;
  commit: (operation: Promise<ArrangementMutationResult | null>) => Promise<CreativeSession | null>;
}

export function ArrangeMidiEditor(props: ArrangeMidiEditorProps) {
  const { api, commit } = props;
  return (
    <MidiEditorPanel
      clip={props.clip}
      timebase={props.timebase}
      ghostNotes={props.ghostNotes}
      playheadTick={props.playheadTick}
      playheadTickRef={props.playheadTickRef}
      playing={props.playing}
      onSeek={props.onSeek}
      previewAvailable={props.previewAvailable}
      onSendMidi={props.onSendMidi}
      onPanicMidi={props.onPanicMidi}
      toolbarTrailing={props.toolbarTrailing}
      onAddNote={(clipId, startTick, pitch, durationTicks, velocity, channel) =>
        commit(
          api.addMidiNote(
            clipId,
            Math.max(0, Math.round(startTick)),
            pitch,
            Math.max(1, Math.round(durationTicks)),
            velocity,
            channel,
          ),
        )
      }
      onUpdateNote={(clipId, note) =>
        commit(
          api.updateMidiNote(clipId, note.id, {
            note: note.note,
            startTick: note.startTick,
            durationTicks: note.durationTicks,
            velocity: note.velocity,
          }),
        )
      }
      onUpdateNotes={(clipId, updates) =>
        commit(
          api.updateMidiNotes(
            clipId,
            updates.map((update) => ({
              noteId: update.noteId,
              patch: {
                note: update.patch.note,
                startTick: update.patch.startTick,
                durationTicks: update.patch.durationTicks,
                velocity: update.patch.velocity,
              },
            })),
          ),
        )
      }
      onRemoveNotes={(clipId, noteIds) => commit(api.removeMidiNotes(clipId, noteIds))}
      onInsertNotes={(clipId, notes) => commit(api.insertMidiNotes(clipId, notes))}
      onQuantize={(clipId, noteIds, gridTicks) =>
        commit(api.quantizeMidiNotes(clipId, noteIds, gridTicks))
      }
      onDuplicateNotes={(clipId, noteIds, offsetTicks) =>
        commit(api.duplicateMidiNotes(clipId, noteIds, offsetTicks))
      }
    />
  );
}
