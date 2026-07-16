import type { Workspace } from '@/lib/domain';
import { Icon } from './ui';

export function EmptyWorkspace({ workspace }: { workspace: Workspace }) {
  const copy: Record<Workspace, { title: string; body: string; action: string }> = {
    home: { title: '', body: '', action: '' },
    play: { title: '', body: '', action: '' },
    arrange: {
      title: 'Timelineへ素材を置く',
      body: 'Recording、Audio、MIDIを非破壊で配置し、同期位置を保ったまま編集します。',
      action: 'Inboxを開く',
    },
    design: {
      title: '素材を設計する',
      body: 'Sample、Analyze、SeparateをひとつのDesign workspaceから切り替えます。',
      action: 'Design toolを選択',
    },
  };
  const item = copy[workspace];
  return (
    <div className="empty-workspace">
      <span className={`empty-orbit orbit-${workspace}`}>
        <i />
        <b />
      </span>
      <span className="eyebrow">{workspace.toUpperCase()} WORKSPACE</span>
      <h1>{item.title}</h1>
      <p>{item.body}</p>
      <button className="primary">
        <Icon name="plus" />
        {item.action}
      </button>
      <small>
        このワークスペースの処理エンジンは後続ゲートで接続されます。Scratch Sessionは維持されます。
      </small>
    </div>
  );
}
