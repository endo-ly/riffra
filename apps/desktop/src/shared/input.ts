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
