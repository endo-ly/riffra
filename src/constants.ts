import type { Workspace } from '@/lib/domain';

export const workspaces: { id: Workspace; label: string; key: string }[] = [
  { id: 'home', label: 'Home', key: '1' },
  { id: 'play', label: 'Play', key: '2' },
  { id: 'arrange', label: 'Arrange', key: '3' },
  { id: 'design', label: 'Design', key: '4' },
];

export const librarySections = ['Plugins', 'Racks', 'Recordings'];
