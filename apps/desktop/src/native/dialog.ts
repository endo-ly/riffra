import { open } from '@tauri-apps/plugin-dialog';

export async function openMidiFile(): Promise<string | null> {
  const result = await open({
    multiple: false,
    filters: [{ name: 'Standard MIDI', extensions: ['mid', 'midi'] }],
  });
  return typeof result === 'string' ? result : null;
}
