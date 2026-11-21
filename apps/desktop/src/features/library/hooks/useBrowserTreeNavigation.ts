import { useEffect, useMemo, useState, type KeyboardEvent } from 'react';
import type {
  BrowserFolder,
  BrowserItem,
  BrowserNode,
  BrowserRow,
} from '@/features/library/model/browser-tree';
import { rowId, visibleRows } from '@/features/library/model/browser-tree';

export function useBrowserTreeNavigation(
  tree: BrowserNode[],
  searchRows: BrowserRow[] | null,
  apply: (item: BrowserItem) => void,
  action: (item: BrowserItem) => void,
  openMenu: (item: BrowserItem, x: number, y: number) => void,
) {
  const [expanded, setExpanded] = useState<ReadonlySet<string>>(() => new Set(['instruments']));
  const [activeRowKey, setActiveRowKey] = useState<string | null>(null);
  const [selectedKey, setSelectedKey] = useState<string | null>(null);
  const rows = useMemo(
    () => searchRows ?? visibleRows(tree, expanded),
    [tree, searchRows, expanded],
  );
  const activeIndex = rows.findIndex((row) => row.key === activeRowKey);
  useEffect(() => {
    if (activeIndex >= 0)
      document.getElementById(rowId(activeIndex))?.scrollIntoView?.({ block: 'nearest' });
  }, [activeIndex]);
  const select = (row: BrowserRow) => {
    setActiveRowKey(row.key);
    if (row.node.kind !== 'folder') setSelectedKey(row.node.key);
  };
  const toggleFolder = (folder: BrowserFolder) =>
    setExpanded((current) => {
      const next = new Set(current);
      if (next.has(folder.key)) next.delete(folder.key);
      else next.add(folder.key);
      return next;
    });
  const onKeyDown = (event: KeyboardEvent<HTMLDivElement>) => {
    const row = rows[activeIndex];
    const moveTo = (next: number) => {
      const target = rows[Math.max(0, Math.min(rows.length - 1, next))];
      if (target) select(target);
    };
    switch (event.key) {
      case 'ArrowDown':
        moveTo(activeIndex + 1);
        break;
      case 'ArrowUp':
        moveTo(activeIndex < 0 ? 0 : activeIndex - 1);
        break;
      case 'Home':
        moveTo(0);
        break;
      case 'End':
        moveTo(rows.length - 1);
        break;
      case 'ArrowRight':
        if (row?.node.kind !== 'folder') return;
        if (expanded.has(row.node.key)) moveTo(activeIndex + 1);
        else toggleFolder(row.node);
        break;
      case 'ArrowLeft':
        if (row?.node.kind === 'folder' && expanded.has(row.node.key)) toggleFolder(row.node);
        else if (row?.parentKey) {
          const parent = rows.find((candidate) => candidate.key === row.parentKey);
          if (parent) select(parent);
        }
        break;
      case 'Enter':
        if (!row) return;
        if (row.node.kind === 'folder') toggleFolder(row.node);
        else apply(row.node);
        break;
      case 'ContextMenu':
      case 'F10': {
        if (!row || row.node.kind === 'folder' || (event.key === 'F10' && !event.shiftKey)) return;
        const bounds = document.getElementById(rowId(activeIndex))?.getBoundingClientRect();
        openMenu(row.node, bounds?.left ?? 0, bounds?.bottom ?? 0);
        break;
      }
      case ' ':
        if (!row) return;
        if (row.node.kind === 'folder') toggleFolder(row.node);
        else action(row.node);
        break;
      default:
        return;
    }
    event.preventDefault();
  };
  return { rows, expanded, activeIndex, selectedKey, select, toggleFolder, onKeyDown };
}
