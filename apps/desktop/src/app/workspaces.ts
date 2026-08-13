import type { Workspace } from '@/model/domain';

export const workspaces: { id: Workspace; label: string; key: string }[] = [
  { id: 'arrange', label: 'Arrange', key: '1' },
  { id: 'design', label: 'Design', key: '2' },
];
