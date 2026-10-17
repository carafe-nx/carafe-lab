import type { ReactNode } from "react";
import { useTranslation } from "react-i18next";
import type { Step } from "@/shared/api/bindings/Step";
import styles from "@/shared/ui/lab.module.css";

export function stepKind(step: Step): string {
  switch (step.step) {
    case "launch":
    case "press":
    case "screenshot":
    case "close":
      return styles.kindAct ?? "";
    case "waitFor":
    case "wait":
      return styles.kindWait ?? "";
    default:
      return styles.kindCheck ?? "";
  }
}

export function StepLine({ step, aside }: { step: Step; aside?: ReactNode }) {
  const { t } = useTranslation();
  return (
    <li className={stepKind(step)}>
      <b>{t(`steps.${step.step}`)}</b>
      <StepParams step={step} />
      {aside}
    </li>
  );
}

function StepParams({ step }: { step: Step }) {
  const { t } = useTranslation();
  const params: string[] = [];
  switch (step.step) {
    case "waitFor":
      params.push(t(`milestones.${step.milestone}`), t("units.seconds", { count: step.timeout }));
      break;
    case "wait":
      params.push(t("units.seconds", { count: step.seconds }));
      break;
    case "press":
      params.push(step.buttons.join(" + "));
      if (step.hold !== 0.15) params.push(t("units.seconds", { count: step.hold }));
      break;
    case "screenshot":
    case "expectPattern":
      params.push(step.name);
      break;
    case "expectChange":
      params.push(`${step.from} → ${step.to}`);
      if (step.quiet?.length) params.push(t("steps.quietCount", { count: step.quiet.length }));
      if (step.soft) params.push(t("steps.soft"));
      break;
    case "close":
      params.push(t("units.seconds", { count: step.timeout }));
      break;
    case "expectPerformance":
      params.push(step.threshold === undefined ? t("steps.gameThreshold") : `${step.threshold} %`);
      break;
    case "randomPresses":
      params.push(t("units.seconds", { count: step.seconds }));
      params.push(`${t("steps.seed")} ${step.seed}`);
      break;
    default:
      break;
  }
  return (
    <>
      {params.map((param) => (
        <span key={param} className={styles.param}>
          {param}
        </span>
      ))}
    </>
  );
}
