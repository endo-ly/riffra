import type { Workspace } from '@/lib/domain';

export const workspaces: { id: Workspace; label: string; key: string }[] = [
  { id: 'home', label: 'Home', key: '1' },
  { id: 'play', label: 'Play', key: '2' },
  { id: 'arrange', label: 'Arrange', key: '3' },
  { id: 'sample', label: 'Sample', key: '4' },
  { id: 'analyze', label: 'Analyze', key: '5' },
  { id: 'separate', label: 'Separate', key: '6' },
];

export const librarySections = [
  'Plugins',
  'Racks',
  'Presets',
  'Samples',
  'Recordings',
  'MIDI',
  'Projects',
  'References',
];

export const DEFAULT_TEMPO_BPM = 120;
export const COUNT_IN_BEAT_MS = 60_000 / DEFAULT_TEMPO_BPM;
