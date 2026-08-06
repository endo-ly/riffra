import type { Workspace } from '@/lib/domain';

export const workspaces: { id: Workspace; label: string; key: string }[] = [
  { id: 'arrange', label: 'Arrange', key: '1' },
  { id: 'play', label: 'Play', key: '2' },
  { id: 'design', label: 'Design', key: '3' },
];

export const librarySections = ['Plugins', 'Racks', 'Recordings'];
