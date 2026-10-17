import { useTranslation } from "react-i18next";
import { StepParams, stepKind } from "@/features/lab/StepLine";
import type { Step } from "@/shared/api/bindings/Step";
import flow from "./ScenarioFlow.module.css";

export function ScenarioFlow({ steps }: { steps: Step[] }) {
  return (
    <div className={flow.flow}>
      <FlowList steps={steps} />
    </div>
  );
}

function FlowList({ steps }: { steps: Step[] }) {
  const { t } = useTranslation();
  if (steps.length === 0) {
    return <span className={flow.empty}>{t("scenarios.noSteps")}</span>;
  }
  return (
    <ol className={flow.list}>
      {steps.map((step, index) => (
        <li key={index} className={flow.item}>
          <FlowStep step={step} />
        </li>
      ))}
    </ol>
  );
}

function FlowStep({ step }: { step: Step }) {
  const { t } = useTranslation();
  if (step.step === "repeat") {
    return (
      <div className={flow.loop}>
        <span className={flow.head}>{t("scenarios.loop", { count: step.times })}</span>
        <FlowList steps={step.steps} />
      </div>
    );
  }
  if (step.step === "ifScreen") {
    return (
      <div className={flow.choice}>
        <span className={flow.question}>{t("scenarios.question", { reference: step.reference || "—" })}</span>
        <div className={flow.branches}>
          <div className={flow.yes}>
            <span className={flow.head}>{t("scenarios.then")}</span>
            <FlowList steps={step.then} />
          </div>
          <div className={flow.no}>
            <span className={flow.head}>{t("scenarios.otherwise")}</span>
            <FlowList steps={step.otherwise} />
          </div>
        </div>
      </div>
    );
  }
  return (
    <div className={`${flow.box} ${stepKind(step)}`}>
      <b>{t(`steps.${step.step}`)}</b>
      <StepParams step={step} />
    </div>
  );
}
