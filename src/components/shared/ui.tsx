export function Icon({ name }: { name: string }) {
  const paths: Record<string, string> = {
    search:
      'M11 4a7 7 0 1 0 4.9 12l4.55 4.55 1.4-1.4-4.55-4.55A7 7 0 0 0 11 4Zm0 2a5 5 0 1 1 0 10 5 5 0 0 1 0-10Z',
    play: 'M8 5v14l11-7Z',
    stop: 'M7 7h10v10H7Z',
    record: 'M12 5a7 7 0 1 0 0 14 7 7 0 0 0 0-14Z',
    loop: 'M7 7h10V4l4 4-4 4V9H7a3 3 0 0 0-3 3v1H2v-1a5 5 0 0 1 5-5Zm10 10H7v3l-4-4 4-4v3h10a3 3 0 0 0 3-3v-1h2v1a5 5 0 0 1-5 5Z',
    plus: 'M11 5h2v6h6v2h-6v6h-2v-6H5v-2h6Z',
    chevron: 'm9 18 6-6-6-6',
    bolt: 'm13 2-9 12h7l-1 8 9-12h-7Z',
    command:
      'M9 6a3 3 0 1 0-3 3h3V6Zm2 0v3h2V6h-2Zm4 0v3h3a3 3 0 1 0-3-3ZM9 11H6a3 3 0 1 0 3 3v-3Zm2 0v2h2v-2h-2Zm4 0v3a3 3 0 1 0 3-3h-3Zm-6 5H6a1 1 0 1 1 1-1h2v1Zm2-1h2v2h-2v-2Zm4 0h2a1 1 0 1 1-2 1v-1Z',
  };
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24">
      <path d={paths[name] ?? paths.plus} />
    </svg>
  );
}

export function Meter({ value, danger = false }: { value: number; danger?: boolean }) {
  return (
    <span className={`meter ${danger ? 'meter-danger' : ''}`}>
      <i style={{ width: `${Math.max(2, Math.min(100, value))}%` }} />
    </span>
  );
}
