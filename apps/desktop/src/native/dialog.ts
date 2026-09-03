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
    defaultPath: `${sanitizeProjectFileName(defaultName)}.riffra`,
    filters: [{ name: 'Riffra Project', extensions: ['riffra'] }],
  });
  return typeof result === 'string' ? result : null;
}

function sanitizeProjectFileName(value: string): string {
  const sanitized = [...value.trim()]
    .map((character) => {
      const codePoint = character.codePointAt(0) ?? 0;
      return codePoint < 0x20 || '<>:"/\\|?*'.includes(character) ? '-' : character;
    })
    .join('')
    .replace(/[. ]+$/g, '');
  if (!sanitized || /^(con|prn|aux|nul|com[1-9]|lpt[1-9])(?:\..*)?$/i.test(sanitized)) {
    return 'Untitled Project';
  }
  return sanitized;
}
