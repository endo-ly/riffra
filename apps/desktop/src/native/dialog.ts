import { open, save } from '@tauri-apps/plugin-dialog';

export async function openMidiFile(): Promise<string | null> {
  const result = await open({
    multiple: false,
    filters: [{ name: 'Standard MIDI', extensions: ['mid', 'midi'] }],
  });
  return typeof result === 'string' ? result : null;
}

export async function openHostDataRoot(): Promise<string | null> {
  const result = await open({ directory: true, multiple: false });
  return typeof result === 'string' ? result : null;
}

export async function openProjectPackage(): Promise<string | null> {
  const result = await open({
    multiple: false,
    filters: [{ name: 'Riffra Project', extensions: ['riffra'] }],
  });
  return typeof result === 'string' ? result : null;
}

export async function saveProjectPackage(defaultName: string): Promise<string | null> {
  const result = await save({
    defaultPath: `${defaultName}.riffra`,
    filters: [{ name: 'Riffra Project', extensions: ['riffra'] }],
  });
  return typeof result === 'string' ? result : null;
}
