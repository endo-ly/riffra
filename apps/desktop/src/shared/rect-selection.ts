interface ClientRect {
  left: number;
  top: number;
  right: number;
  bottom: number;
}

/** Returns the ids of the elements under `container` whose bounds touch `rect`. */
export function idsInClientRect(
  container: ParentNode,
  selector: string,
  readId: (element: HTMLElement) => string | undefined,
  rect: ClientRect,
): string[] {
  return Array.from(container.querySelectorAll<HTMLElement>(selector))
    .filter((element) => {
      const bounds = element.getBoundingClientRect();
      return (
        bounds.right >= rect.left &&
        bounds.left <= rect.right &&
        bounds.bottom >= rect.top &&
        bounds.top <= rect.bottom
      );
    })
    .map(readId)
    .filter((id): id is string => id !== undefined);
}
