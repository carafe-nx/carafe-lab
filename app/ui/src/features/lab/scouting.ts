import type { Scenario } from "@/shared/api/bindings/Scenario";
import type { Step } from "@/shared/api/bindings/Step";
import type { SwitchButton } from "@/shared/api/bindings/SwitchButton";

export const KEY_BUTTONS: Record<string, SwitchButton> = {
  c: "A",
  x: "B",
  v: "X",
  z: "Y",
  q: "L",
  e: "R",
  r: "ZL",
  t: "ZR",
  m: "PLUS",
  n: "MINUS",
  f: "STICKL",
  g: "STICKR",
  arrowup: "UP",
  arrowdown: "DOWN",
  arrowleft: "LEFT",
  arrowright: "RIGHT",
  w: "LUP",
  s: "LDOWN",
  a: "LLEFT",
  d: "LRIGHT",
  i: "RUP",
  k: "RDOWN",
  j: "RLEFT",
  l: "RRIGHT",
};

export type Pressed = { button: SwitchButton; down: number; up: number };
export type Mark = { name: string; at: number };

const CHORD_MS = 80;
const SHORTEST_WAIT = 0.1;

type Item = { at: number; until: number; step: Step };

function seconds(ms: number): number {
  return Math.round(ms / 100) / 10;
}

export function scoutedSteps(presses: Pressed[], marks: Mark[]): Step[] {
  const sorted = [...presses].sort((a, b) => a.down - b.down);
  const items: Item[] = [];
  for (const press of sorted) {
    const chord = items.find(
      (item) => item.step.step === "press" && press.down - item.at <= CHORD_MS && !item.step.buttons.includes(press.button),
    );
    if (chord && chord.step.step === "press") {
      chord.step.buttons.push(press.button);
      chord.until = Math.max(chord.until, press.up);
      chord.step.hold = Math.max(SHORTEST_WAIT, seconds(chord.until - chord.at));
    } else {
      items.push({
        at: press.down,
        until: press.up,
        step: { step: "press", buttons: [press.button], hold: Math.max(SHORTEST_WAIT, seconds(press.up - press.down)) },
      });
    }
  }
  for (const mark of marks) {
    items.push({ at: mark.at, until: mark.at, step: { step: "screenshot", name: mark.name } });
  }
  items.sort((a, b) => a.at - b.at);
  const steps: Step[] = [];
  let cursor = 0;
  for (const item of items) {
    const gap = seconds(item.at - cursor);
    if (gap >= SHORTEST_WAIT) {
      steps.push({ step: "wait", seconds: gap });
    }
    steps.push(item.step);
    cursor = Math.max(cursor, item.until);
  }
  return steps;
}

export function scoutedScenario(presses: Pressed[], marks: Mark[]): Scenario {
  return {
    steps: [
      { step: "launch" },
      { step: "waitFor", milestone: "firstFrame", timeout: 120 },
      ...scoutedSteps(presses, marks),
      { step: "close", timeout: 60 },
    ],
  };
}
