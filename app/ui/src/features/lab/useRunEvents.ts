import { useEffect, useRef, useState } from "react";
import type { Event } from "@/shared/api/bindings/Event";
import { api } from "@/shared/api/commands";

export function useRunEvents(dir: string | null, live: boolean): Event[] {
  const [events, setEvents] = useState<Event[]>([]);
  const count = useRef(0);

  useEffect(() => {
    setEvents([]);
    count.current = 0;
    if (!dir) {
      return;
    }
    let active = true;
    let busy = false;
    const load = () => {
      if (busy) return;
      busy = true;
      api
        .runEvents(dir, count.current)
        .then((fresh) => {
          if (active && fresh.length > 0) {
            count.current += fresh.length;
            setEvents((known) => [...known, ...fresh]);
          }
        })
        .catch(() => {})
        .finally(() => {
          busy = false;
        });
    };
    load();
    if (!live) {
      return () => {
        active = false;
      };
    }
    const timer = window.setInterval(load, 1500);
    return () => {
      active = false;
      window.clearInterval(timer);
    };
  }, [dir, live]);

  return events;
}

export function folderName(path: string): string {
  const parts = path.split(/[\\/]/).filter(Boolean);
  return parts[parts.length - 1] ?? path;
}
