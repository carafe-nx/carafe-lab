import { invoke } from "@tauri-apps/api/core";
import type { CommandError } from "./bindings/CommandError";
import type { Event } from "./bindings/Event";
import type { FirmwareState } from "./bindings/FirmwareState";
import type { GameProblems } from "./bindings/GameProblems";
import type { KeysState } from "./bindings/KeysState";
import type { LabStatus } from "./bindings/LabStatus";
import type { LabView } from "./bindings/LabView";
import type { PerfSample } from "./bindings/PerfSample";
import type { RunRequest } from "./bindings/RunRequest";
import type { RunSummary } from "./bindings/RunSummary";
import type { Scenario } from "./bindings/Scenario";
import type { Screen } from "./bindings/Screen";
import type { VersionStep } from "./bindings/VersionStep";

export const api = {
  getStatus: () => invoke<LabStatus>("get_status"),
  gameProblems: (dir: string, game: string) => invoke<GameProblems>("game_problems", { dir, game }),
  versionSteps: () => invoke<VersionStep[]>("version_steps"),
  getLab: () => invoke<LabView>("get_lab"),
  listRuns: () => invoke<RunSummary[]>("list_runs"),
  runEvents: (dir: string, from: number) => invoke<Event[]>("run_events", { dir, from }),
  startRun: (
    set: string | null,
    only: string | null,
    games: string[] | null = null,
    repeat = 1,
    ab: string | null = null,
    clean = false,
    scenario: string | null = null,
    video: "full" | "low" | "off" | null = null,
    like: string | null = null,
  ) => {
    const request: RunRequest = { set, only, games, repeat, ab, clean, scenario, video, like };
    return invoke<void>("start_run", { request });
  },
  stopRun: (id: string | null) => invoke<void>("stop_run", { id }),
  runScreenshots: (dir: string, game: string) => invoke<string[]>("run_screenshots", { dir, game }),
  addGame: (folder: string) => invoke<string>("add_game", { folder }),
  chooseKeys: (path: string) => invoke<KeysState>("choose_keys", { path }),
  forgetKeys: () => invoke<KeysState>("forget_keys"),
  chooseFirmware: (path: string) => invoke<FirmwareState>("choose_firmware", { path }),
  forgetFirmware: () => invoke<FirmwareState>("forget_firmware"),
  openPath: (path: string) => invoke<void>("open_path", { path }),
  readScreen: () => invoke<Screen | null>("read_screen"),
  livePerf: (seconds: number) => invoke<PerfSample[]>("live_perf", { seconds }),
  readImage: (path: string) => invoke<string>("read_image", { path }),
  saveScenario: (game: string, scenario: Scenario) => invoke<string>("save_scenario", { game, scenario }),
  saveGameNotes: (game: string, tags: string[], notes: string) =>
    invoke<void>("save_game_notes", { game, tags, notes }),
};

export function errorText(error: unknown): string {
  if (typeof error === "object" && error !== null && "message" in error) {
    return (error as CommandError).message;
  }
  return String(error);
}
