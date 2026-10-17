import { useCallback, useEffect, useRef, useState } from "react";
import { errorText } from "./commands";

export type Polled<T> = {
  data: T | null;
  error: string | null;
  refresh: () => void;
};

export function usePolling<T>(load: () => Promise<T>, intervalMs: number): Polled<T> {
  const [data, setData] = useState<T | null>(null);
  const [error, setError] = useState<string | null>(null);
  const latest = useRef(load);
  latest.current = load;
  const busy = useRef(false);

  const refresh = useCallback(() => {
    if (busy.current) {
      return;
    }
    busy.current = true;
    latest
      .current()
      .then((value) => {
        setData(value);
        setError(null);
      })
      .catch((reason: unknown) => setError(errorText(reason)))
      .finally(() => {
        busy.current = false;
      });
  }, []);

  useEffect(() => {
    refresh();
    if (intervalMs <= 0) {
      return;
    }
    const timer = window.setInterval(refresh, intervalMs);
    return () => window.clearInterval(timer);
  }, [refresh, intervalMs]);

  return { data, error, refresh };
}
