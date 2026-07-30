import { useCallback, useState } from 'react';

export function useInspectorOperation() {
  const [operationMessage, setOperationMessage] = useState<string | null>(null);
  const runOperation = useCallback(
    <T>(operation: Promise<T>, successMessage: string, apply?: (value: T) => void) => {
      setOperationMessage(null);
      void operation
        .then((value) => {
          apply?.(value);
          setOperationMessage(successMessage);
        })
        .catch((error: unknown) => {
          setOperationMessage(error instanceof Error ? error.message : String(error));
        });
    },
    [],
  );
  return { operationMessage, runOperation, setOperationMessage };
}
