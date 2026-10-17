import { invoke } from "@tauri-apps/api/core";
import type { CommandError } from "./bindings/CommandError";
import type { Event } from "./bindings/Event";
import type { KeysState } from "./bindings/KeysState";
import type { LabStatus } from "./bindings/LabStatus";
import type { LabView } from "./bindings/LabView";
import type { RunSummary } from "./bindings/RunSummary";
import type { Screen } from "./bindings/Screen";

export const api = {
  getStatus: () => invoke<LabStatus>("get_status"),
  getLab: () => invoke<LabView>("get_lab"),
  listRuns: () => invoke<RunSummary[]>("list_runs"),
  runEvents: (dir: string, from: number) => invoke<Event[]>("run_events", { dir, from }),
  startRun: (set: string | null, only: string | null) => invoke<void>("start_run", { set, only }),
  stopRun: (id: string | null) => invoke<void>("stop_run", { id }),
  chooseKeys: (path: string) => invoke<KeysState>("choose_keys", { path }),
  forgetKeys: () => invoke<KeysState>("forget_keys"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  readScreen: () => invoke<Screen | null>("read_screen"),
  readImage: (path: string) => invoke<string>("read_image", { path }),
};

export function errorText(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    return (error as CommandError).message;
  }
  return String(error);
}
