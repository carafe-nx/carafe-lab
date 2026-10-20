import { useTranslation } from "react-i18next";
import { stepKind } from "@/features/lab/StepLine";
import type { Milestone } from "@/shared/api/bindings/Milestone";
import type { Step } from "@/shared/api/bindings/Step";
import type { SwitchButton } from "@/shared/api/bindings/SwitchButton";
import { Button } from "@/shared/ui/Button";
import styles from "./ScenariosPage.module.css";

export const BUTTONS: SwitchButton[] = [
  "A", "B", "X", "Y", "L", "R", "ZL", "ZR", "PLUS", "MINUS", "STICKL", "STICKR",
  "UP", "DOWN", "LEFT", "RIGHT", "LUP", "LDOWN", "LLEFT", "LRIGHT", "RUP", "RDOWN", "RLEFT", "RRIGHT",
];

const MILESTONES: Milestone[] = ["wine", "loadingScreen", "firstFrame", "commit", "programEnded", "errorScreen"];

export const NEW_STEPS: Step["step"][] = [
  "launch", "waitFor", "wait", "press", "screenshot", "stacks", "expectChange",
  "expectPattern", "expectScreen", "expectInput", "expectPointer", "expectThreads", "expectSave", "expectSaveKept", "expectPerformance", "randomPresses",
  "repeat", "ifScreen", "close",
];

export function newStep(kind: Step["step"], steps: Step[]): Step {
  const shots = steps.flatMap((step) => (step.step === "screenshot" ? [step.name] : []));
  switch (kind) {
    case "launch":
      return { step: "launch" };
    case "waitFor":
      return { step: "waitFor", milestone: "firstFrame", timeout: 120 };
    case "wait":
      return { step: "wait", seconds: 5 };
    case "press":
      return { step: "press", buttons: ["A"], hold: 0.15 };
    case "screenshot":
      return { step: "screenshot", name: `shot-${shots.length + 1}` };
    case "stacks":
      return { step: "stacks", name: `stacks-${steps.filter((step) => step.step === "stacks").length + 1}` };
    case "expectChange":
      return { step: "expectChange", from: shots[shots.length - 2] ?? "", to: shots[shots.length - 1] ?? "", soft: true };
    case "expectPattern":
      return { step: "expectPattern", name: shots[shots.length - 1] ?? "" };
    case "expectScreen":
      return { step: "expectScreen", name: `screen-${shots.length + 1}`, reference: "", tolerance: 12 };
    case "expectInput":
      return { step: "expectInput" };
    case "expectPointer":
      return { step: "expectPointer" };
    case "expectThreads":
      return { step: "expectThreads", min: 30 };
    case "expectSave":
      return { step: "expectSave" };
    case "expectSaveKept":
      return { step: "expectSaveKept" };
    case "expectPerformance":
      return { step: "expectPerformance" };
    case "randomPresses":
      return { step: "randomPresses", seconds: 60, seed: 1 };
    case "repeat":
      return { step: "repeat", times: 3, steps: [{ step: "press", buttons: ["A"], hold: 0.15 }] };
    case "ifScreen":
      return { step: "ifScreen", reference: "", tolerance: 12, then: [], otherwise: [] };
    case "close":
      return { step: "close", timeout: 60 };
  }
}

type StepEditorProps = {
  step: Step;
  index: number;
  count: number;
  shots: string[];
  onChange: (step: Step) => void;
  onMove: (offset: number) => void;
  onRemove: () => void;
};

export function StepEditor({ step, index, count, shots, onChange, onMove, onRemove }: StepEditorProps) {
  const { t } = useTranslation();
  const id = `step-${index}`;
  return (
    <li className={`${styles.step} ${stepKind(step)}`}>
      <span className={styles.number}>{index + 1}</span>
      <b className={styles.kind}>{t(`steps.${step.step}`)}</b>
      <span className={styles.params}>
        <Params step={step} id={id} shots={shots} onChange={onChange} />
      </span>
      <span className={styles.tools}>
        <Button variant="ghost" disabled={index === 0} onClick={() => onMove(-1)} aria-label={t("scenarios.up")}>
          ↑
        </Button>
        <Button variant="ghost" disabled={index === count - 1} onClick={() => onMove(1)} aria-label={t("scenarios.down")}>
          ↓
        </Button>
        <Button variant="ghost" onClick={onRemove} aria-label={t("scenarios.remove")}>
          ✕
        </Button>
      </span>
      {step.step === "repeat" ? (
        <div className={styles.nested}>
          <StepList steps={step.steps} shots={shots} nested onChange={(steps) => onChange({ ...step, steps })} />
        </div>
      ) : null}
      {step.step === "ifScreen" ? (
        <div className={styles.nested}>
          <span className={styles.branch}>{t("scenarios.then")}</span>
          <StepList steps={step.then} shots={shots} nested onChange={(then) => onChange({ ...step, then })} />
          <span className={styles.branch}>{t("scenarios.otherwise")}</span>
          <StepList
            steps={step.otherwise}
            shots={shots}
            nested
            onChange={(otherwise) => onChange({ ...step, otherwise })}
          />
        </div>
      ) : null}
    </li>
  );
}

const OUTSIDE_BLOCKS: Step["step"][] = ["launch", "close"];

export function allShots(steps: Step[]): string[] {
  return steps.flatMap((step) => {
    switch (step.step) {
      case "screenshot":
      case "expectScreen":
        return [step.name];
      case "repeat":
        return allShots(step.steps);
      case "ifScreen":
        return [...allShots(step.then), ...allShots(step.otherwise)];
      default:
        return [];
    }
  });
}

type StepListProps = {
  steps: Step[];
  shots: string[];
  nested?: boolean;
  onChange: (steps: Step[]) => void;
};

export function StepList({ steps, shots, nested = false, onChange }: StepListProps) {
  const { t } = useTranslation();
  const move = (index: number, offset: number) => {
    const next = [...steps];
    const [taken] = next.splice(index, 1);
    if (taken) next.splice(index + offset, 0, taken);
    onChange(next);
  };
  const kinds = nested ? NEW_STEPS.filter((kind) => !OUTSIDE_BLOCKS.includes(kind)) : NEW_STEPS;
  return (
    <>
      <ol className={styles.steps}>
        {steps.map((step, index) => (
          <StepEditor
            key={index}
            step={step}
            index={index}
            count={steps.length}
            shots={shots}
            onChange={(changed) => onChange(steps.map((old, at) => (at === index ? changed : old)))}
            onMove={(offset) => move(index, offset)}
            onRemove={() => onChange(steps.filter((_, at) => at !== index))}
          />
        ))}
      </ol>
      {nested ? (
        <select
          className={styles.select}
          aria-label={t("scenarios.add")}
          value=""
          onChange={(event) => {
            const kind = event.target.value as Step["step"];
            if (kind) onChange([...steps, newStep(kind, steps)]);
          }}
        >
          <option value="">+ {t("scenarios.add")}</option>
          {kinds.map((kind) => (
            <option key={kind} value={kind}>
              {t(`steps.${kind}`)}
            </option>
          ))}
        </select>
      ) : (
        <>
          <span className={styles.addLabel}>{t("scenarios.add")}</span>
          <div className={styles.palette}>
            {kinds.map((kind) => (
              <Button key={kind} onClick={() => onChange([...steps, newStep(kind, steps)])}>
                + {t(`steps.${kind}`)}
              </Button>
            ))}
          </div>
        </>
      )}
    </>
  );
}

function NumberInput({ id, value, step, onChange }: { id: string; value: number; step?: number; onChange: (value: number) => void }) {
  return (
    <input
      id={id}
      className={styles.number_input}
      type="number"
      min={0}
      step={step ?? 1}
      value={value}
      onChange={(event) => onChange(Math.max(0, Number(event.target.value) || 0))}
    />
  );
}

function ShotSelect({ id, value, shots, onChange }: { id: string; value: string; shots: string[]; onChange: (value: string) => void }) {
  return (
    <select id={id} className={styles.select} value={value} onChange={(event) => onChange(event.target.value)}>
      {shots.includes(value) ? null : <option value={value}>{value || "—"}</option>}
      {shots.map((shot) => (
        <option key={shot} value={shot}>
          {shot}
        </option>
      ))}
    </select>
  );
}

function QuietPairs({
  id,
  pairs,
  shots,
  onChange,
}: {
  id: string;
  pairs: [string, string][];
  shots: string[];
  onChange: (pairs: [string, string][]) => void;
}) {
  const { t } = useTranslation();
  const replace = (index: number, pair: [string, string]) => onChange(pairs.map((entry, at) => (at === index ? pair : entry)));
  return (
    <>
      <span>{t("steps.quiet")}</span>
      {pairs.map(([first, second], index) => (
        <span key={index} className={styles.pair}>
          <ShotSelect id={`${id}-quiet-${index}-a`} value={first} shots={shots} onChange={(value) => replace(index, [value, second])} />
          <span>→</span>
          <ShotSelect id={`${id}-quiet-${index}-b`} value={second} shots={shots} onChange={(value) => replace(index, [first, value])} />
          <button type="button" className={styles.remove} onClick={() => onChange(pairs.filter((_, at) => at !== index))}>
            ×
          </button>
        </span>
      ))}
      <button type="button" className={styles.add} onClick={() => onChange([...pairs, [shots[0] ?? "", shots[1] ?? ""]])}>
        {t("steps.addQuiet")}
      </button>
    </>
  );
}

function Params({ step, id, shots, onChange }: { step: Step; id: string; shots: string[]; onChange: (step: Step) => void }) {
  const { t } = useTranslation();
  switch (step.step) {
    case "waitFor":
      return (
        <>
          <select
            id={`${id}-milestone`}
            className={styles.select}
            value={step.milestone}
            onChange={(event) => onChange({ ...step, milestone: event.target.value as Milestone })}
          >
            {MILESTONES.map((milestone) => (
              <option key={milestone} value={milestone}>
                {t(`milestones.${milestone}`)}
              </option>
            ))}
          </select>
          <label htmlFor={`${id}-timeout`}>{t("scenarios.upTo")}</label>
          <NumberInput id={`${id}-timeout`} value={step.timeout} onChange={(timeout) => onChange({ ...step, timeout })} />
          <span>{t("scenarios.sec")}</span>
        </>
      );
    case "wait":
      return (
        <>
          <NumberInput id={`${id}-seconds`} value={step.seconds} step={0.5} onChange={(seconds) => onChange({ ...step, seconds })} />
          <span>{t("scenarios.sec")}</span>
        </>
      );
    case "press":
      return (
        <>
          <span className={styles.buttons}>
            {BUTTONS.map((button) => (
              <button
                key={button}
                type="button"
                className={step.buttons.includes(button) ? `${styles.pad} ${styles.padOn}` : styles.pad}
                onClick={() => {
                  const buttons = step.buttons.includes(button)
                    ? step.buttons.filter((held) => held !== button)
                    : [...step.buttons, button];
                  if (buttons.length > 0) onChange({ ...step, buttons });
                }}
              >
                {button}
              </button>
            ))}
          </span>
          <label htmlFor={`${id}-hold`}>{t("scenarios.hold")}</label>
          <NumberInput id={`${id}-hold`} value={step.hold} step={0.05} onChange={(hold) => onChange({ ...step, hold })} />
          <span>{t("scenarios.sec")}</span>
        </>
      );
    case "screenshot":
    case "stacks":
      return (
        <input
          id={`${id}-name`}
          className={styles.text}
          value={step.name}
          onChange={(event) => onChange({ ...step, name: event.target.value.replace(/\s+/g, "-") })}
        />
      );
    case "expectScreen":
      return (
        <>
          <input
            id={`${id}-name`}
            className={styles.text}
            aria-label={t("steps.shotName")}
            value={step.name}
            onChange={(event) => onChange({ ...step, name: event.target.value.replace(/\s+/g, "-") })}
          />
          <label htmlFor={`${id}-reference`}>{t("steps.reference")}</label>
          <input
            id={`${id}-reference`}
            className={styles.text}
            placeholder="openttd/menu.png"
            value={step.reference}
            onChange={(event) => onChange({ ...step, reference: event.target.value.trim() })}
          />
          <label htmlFor={`${id}-tolerance`}>{t("steps.tolerance")}</label>
          <NumberInput id={`${id}-tolerance`} value={step.tolerance} onChange={(tolerance) => onChange({ ...step, tolerance })} />
        </>
      );
    case "expectChange":
      return (
        <>
          <ShotSelect id={`${id}-from`} value={step.from} shots={shots} onChange={(from) => onChange({ ...step, from })} />
          <span>→</span>
          <ShotSelect id={`${id}-to`} value={step.to} shots={shots} onChange={(to) => onChange({ ...step, to })} />
          <QuietPairs id={id} pairs={step.quiet ?? []} shots={shots} onChange={(quiet) => onChange({ ...step, quiet })} />
          <label className={styles.check} htmlFor={`${id}-soft`}>
            <input
              id={`${id}-soft`}
              type="checkbox"
              checked={step.soft}
              onChange={(event) => onChange({ ...step, soft: event.target.checked })}
            />
            {t("steps.soft")}
          </label>
        </>
      );
    case "expectPattern":
      return <ShotSelect id={`${id}-name`} value={step.name} shots={shots} onChange={(name) => onChange({ ...step, name })} />;
    case "close":
      return (
        <>
          <label htmlFor={`${id}-timeout`}>{t("scenarios.upTo")}</label>
          <NumberInput id={`${id}-timeout`} value={step.timeout} onChange={(timeout) => onChange({ ...step, timeout })} />
          <span>{t("scenarios.sec")}</span>
        </>
      );
    case "expectPerformance":
      return (
        <>
          <label htmlFor={`${id}-threshold`}>{t("steps.threshold")}</label>
          <NumberInput
            id={`${id}-threshold`}
            value={step.threshold ?? 20}
            onChange={(threshold) => onChange({ ...step, threshold })}
          />
          <span>%</span>
        </>
      );
    case "randomPresses":
      return (
        <>
          <label htmlFor={`${id}-seconds`}>{t("scenarios.for")}</label>
          <NumberInput id={`${id}-seconds`} value={step.seconds} onChange={(seconds) => onChange({ ...step, seconds })} />
          <span>{t("scenarios.sec")}</span>
          <label htmlFor={`${id}-seed`}>{t("steps.seed")}</label>
          <NumberInput id={`${id}-seed`} value={step.seed} onChange={(seed) => onChange({ ...step, seed })} />
        </>
      );
    case "expectThreads":
      return (
        <>
          <label htmlFor={`${id}-min`}>{t("steps.atLeast")}</label>
          <NumberInput id={`${id}-min`} value={step.min} onChange={(min) => onChange({ ...step, min })} />
        </>
      );
    case "repeat":
      return (
        <>
          <NumberInput id={`${id}-times`} value={step.times} onChange={(times) => onChange({ ...step, times })} />
          <span>{t("scenarios.times")}</span>
        </>
      );
    case "ifScreen":
      return (
        <>
          <label htmlFor={`${id}-reference`}>{t("steps.reference")}</label>
          <input
            id={`${id}-reference`}
            className={styles.text}
            placeholder="openttd/menu.png"
            value={step.reference}
            onChange={(event) => onChange({ ...step, reference: event.target.value.trim() })}
          />
          <label htmlFor={`${id}-tolerance`}>{t("steps.tolerance")}</label>
          <NumberInput id={`${id}-tolerance`} value={step.tolerance} onChange={(tolerance) => onChange({ ...step, tolerance })} />
        </>
      );
    default:
      return null;
  }
}
