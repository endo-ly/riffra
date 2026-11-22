/** Returns whether a keyboard event originated in a control that owns text input. */
export function isEditableTypingTarget(target: EventTarget | null): boolean {
  const element = target as HTMLElement | null;
  return Boolean(
    element &&
    (element.tagName === 'INPUT' ||
      element.tagName === 'TEXTAREA' ||
      element.tagName === 'SELECT' ||
      element.isContentEditable ||
      (typeof element.closest === 'function' &&
        Boolean(element.closest('[contenteditable="true"]')))),
  );
}

/** Wheel delta the WebView reports for one mouse-wheel notch. */
const WHEEL_NOTCH_PIXELS = 100;

/**
 * Scrolls `element` by `pixelsPerNotch` per notch of a plain vertical wheel,
 * replacing the WebView's fixed distance. Modified and horizontal wheels stay
 * with the caller or the native scroll.
 */
export function scrollWheelVertically(
  element: HTMLElement,
  event: WheelEvent,
  pixelsPerNotch: number,
): void {
  if (event.ctrlKey || event.metaKey || event.altKey || event.shiftKey) return;
  if (Math.abs(event.deltaY) <= Math.abs(event.deltaX)) return;
  event.preventDefault();
  element.scrollTop += (event.deltaY / WHEEL_NOTCH_PIXELS) * pixelsPerNotch;
}
