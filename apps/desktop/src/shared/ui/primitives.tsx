import styles from './Meter.module.css';

export function Icon({ name, className }: { name: string; className?: string }) {
  const paths: Record<string, string> = {
    search:
      'M11 4a7 7 0 1 0 4.9 12l4.55 4.55 1.4-1.4-4.55-4.55A7 7 0 0 0 11 4Zm0 2a5 5 0 1 1 0 10 5 5 0 0 1 0-10Z',
    stop: 'M7 7h10v10H7Z',
    undo: 'M9 7H5.8l2.6-2.6L7 3 2 8l5 5 1.4-1.4L5.8 9H9a6 6 0 1 1-5.6 8h2.1A4 4 0 1 0 9 7Z',
    redo: 'M15 7h3.2l-2.6-2.6L17 3l5 5-5 5-1.4-1.4 2.6-2.6H15a6 6 0 1 0 5.6 8h-2.1A4 4 0 1 1 15 7Z',
    plus: 'M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6Z',
    chevron: 'm9 18 6-6-6-6',
    close:
      'M6.4 5 12 10.6 17.6 5 19 6.4 13.4 12l5.6 5.6-1.4 1.4-5.6-5.6L6.4 19 5 17.6l5.6-5.6L5 6.4 6.4 5Z',
    command:
      'M9 6a3 3 0 1 0-3 3h3V6Zm2 0v3h2V6h-2Zm4 0v3h3a3 3 0 1 0-3-3ZM9 11H6a3 3 0 1 0 3 3v-3Zm2 0v2h2v-2h-2Zm4 0v3a3 3 0 1 0 3-3h-3Zm-6 5H6a1 1 0 1 1 1-1h2v1Zm2-1h2v2h-2v-2Zm4 0h2a1 1 0 1 1-2 1v-1Z',
    pointer: 'm6 3 12 9-5.2 1.1 3.5 6.1-2.1 1.2-3.5-6.1L7 18.6 6 3Z',
    pencil:
      'm4 16.8-.8 3.8 3.8-.8L18.7 8.1l-3-3L4 16.8Zm12.4-13.1 3 3 1.1-1.1a1.4 1.4 0 0 0 0-2l-1-1a1.4 1.4 0 0 0-2 0l-1.1 1.1Z',
    scissors:
      'M8 5a3 3 0 1 0 0 6 3 3 0 0 0 0-6Zm0 2a1 1 0 1 1 0 2 1 1 0 0 1 0-2Zm0 6a3 3 0 1 0 0 6 3 3 0 0 0 0-6Zm0 2a1 1 0 1 1 0 2 1 1 0 0 1 0-2Zm2-4 10-6v2l-8 5 8 5v2l-10-6v-2Z',
    zoomOut: 'M5 11h14v2H5v-2Z',
    zoomIn: 'M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6V5Z',
    collapse: 'm5 9 7 7 7-7-1.4-1.4L12 13.2 6.4 7.6 5 9Z',
    expand: 'm5 15 1.4 1.4L12 11.8l5.6 4.6L19 15l-7-7-7 7Z',
    maximize: 'M4 4h6v2H6v4H4V4Zm10 0h6v6h-2V6h-4V4ZM4 14h2v4h4v2H4v-6Zm14 0h2v6h-6v-2h4v-4Z',
    restore: 'M7 7h10v10H7V7Zm2 2v6h6V9H9Z',
    curve:
      'M2.7 15.6C5.7 7.2 9.4 6.3 12.4 10.2C15 13.6 17.5 13.7 21.3 6.4L22.9 7.2C18.7 15.7 15 16.5 11.9 12.5C9.3 9.2 7 9.6 4.3 17.1Z',
    magnet: 'M5 3h5v8a2 2 0 0 0 4 0V3h5v8a7 7 0 0 1-14 0V3Z',
    copy: 'M8 8h12v12H8V8ZM4 4h10v2H6v8H4V4Z',
    speaker: 'M4 9h4l5-4v14l-5-4H4V9Zm11.6-.5a6.3 6.3 0 0 1 0 7l-1.6-1.2a4.3 4.3 0 0 0 0-4.6Z',
    module: 'M4 4h7v7H4V4Zm9 0h7v7h-7V4ZM4 13h7v7H4v-7Zm9 0h7v7h-7v-7Z',
    wave: 'M3 10h2v4H3v-4Zm4-3h2v10H7V7Zm4-4h2v18h-2V3Zm4 4h2v10h-2V7Zm4 3h2v4h-2v-4Z',
    note: 'M12 3v10.55A4 4 0 1 0 14 17V7h4V3h-6Z',
    keys: 'M4 17h16v2H4v-2Zm0-11h3v9H4V6Zm6.5 0h3v9h-3V6Zm6.5 0h3v9h-3V6Z',
    grip: 'M9 5h2v2H9V5Zm4 0h2v2h-2V5ZM9 11h2v2H9v-2Zm4 0h2v2h-2v-2ZM9 17h2v2H9v-2Zm4 0h2v2h-2v-2Z',
    more: 'M6 10a2 2 0 1 1 0 4 2 2 0 0 1 0-4Zm6 0a2 2 0 1 1 0 4 2 2 0 0 1 0-4Zm6 0a2 2 0 1 1 0 4 2 2 0 0 1 0-4Z',
    import: 'M12 3 6.6 8.4 8 9.8l3-3V15h2V6.8l3 3 1.4-1.4L12 3ZM5 18h14v2H5v-2Z',
  };
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className={className}>
      <path d={paths[name] ?? paths.plus} />
    </svg>
  );
}

export function Meter({
  value,
  danger = false,
  className,
}: {
  value: number;
  danger?: boolean;
  className?: string;
}) {
  return (
    <span className={`${styles.meter} ${className ?? ''} ${danger ? styles.danger : ''}`}>
      <i style={{ width: `${Math.max(2, Math.min(100, value))}%` }} />
    </span>
  );
}
