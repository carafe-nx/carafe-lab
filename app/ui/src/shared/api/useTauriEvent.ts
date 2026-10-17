import { listen } from "@tauri-apps/api/event";
import { useEffect, useRef } from "react";
import type { RunExit } from "./bindings/RunExit";

type EventMap = {
  "run://exited": RunExit;
};

export function useTauriEvent<K extends keyof EventMap>(name: K, handler: (payload: EventMap[K]) => void) {
  const latest = useRef(handler);
  latest.current = handler;
  useEffect(() => {
    let active = true;
    const pending = listen<EventMap[K]>(name, (event) => {
      if (active) {
        latest.current(event.payload);
      }
    });
    return () => {
      active = false;
      void pending.then((unlisten) => unlisten());
    };
  }, [name]);
}
