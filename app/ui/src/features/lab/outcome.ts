import type { Outcome } from "@/shared/api/bindings/Outcome";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";

export type Tone = "ok" | "warn" | "bad" | "acc" | "plain";

export function outcomeTone(outcome: Outcome): Tone {
  switch (outcome) {
    case "pass":
      return "ok";
    case "skip":
      return "plain";
    case "warn":
      return "warn";
    case "console":
      return "acc";
    case "fail":
      return "bad";
  }
}

export function runCounts(run: RunSummary): { passed: number; warned: number; console: number; failed: number } {
  const counts = { passed: 0, warned: 0, console: 0, failed: 0 };
  for (const game of run.games) {
    switch (game.outcome) {
      case "pass":
        counts.passed += 1;
        break;
      case "warn":
        counts.warned += 1;
        break;
      case "console":
        counts.console += 1;
        break;
      case "fail":
        counts.failed += 1;
        break;
      case "skip":
        break;
    }
  }
  return counts;
}

export function lastResult(
  runs: RunSummary[],
  game: string,
): { run: RunSummary; outcome: Outcome; seconds: number } | null {
  for (const run of runs) {
    const found = run.games.find(
      (entry) => entry.name.toLowerCase() === game.toLowerCase() && entry.outcome !== "skip",
    );
    if (found) {
      return { run, outcome: found.outcome, seconds: found.seconds };
    }
  }
  return null;
}

export function gameRecord(runs: RunSummary[], game: string, limit = 10): Outcome[] {
  const outcomes: Outcome[] = [];
  for (const run of runs) {
    for (const entry of run.games) {
      const counts = entry.name.toLowerCase() === game.toLowerCase() && entry.outcome !== "skip";
      if (counts && outcomes.length < limit) {
        outcomes.push(entry.outcome);
      }
    }
  }
  return outcomes;
}

export function lastSeconds(runs: RunSummary[], game: string): number | null {
  for (const run of runs) {
    const found = run.games.find((entry) => entry.name.toLowerCase() === game.toLowerCase() && entry.seconds > 0);
    if (found) {
      return found.seconds;
    }
  }
  return null;
}

export function runPath(work: string, dir: string, ...rest: string[]): string {
  return [work, "runs", dir, ...rest].join("\\");
}
