import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useLab } from "@/features/lab/LabProvider";
import { StepLine } from "@/features/lab/StepLine";
import { Chip } from "@/shared/ui/Chip";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";

export function ScenariosPage() {
  const { t } = useTranslation();
  const { lab } = useLab();
  const [picked, setPicked] = useState<string | null>(null);
  const games = lab.data?.games ?? [];
  const selected = games.find((game) => game.name === picked) ?? games[0] ?? null;
  return (
    <Page title={t("nav.scenarios")}>
      {lab.error ? <Notice tone="error">{lab.error}</Notice> : null}
      <p className={styles.muted}>{t("scenarios.hint")}</p>
      <div className={styles.split}>
        <div className={styles.card}>
          {selected ? (
            <>
              <div className={styles.cardHead}>
                <h2>{selected.name}</h2>
                <Chip tone={selected.ownScenario ? "acc" : "plain"}>
                  {selected.ownScenario ? t("scenarios.own") : t("scenarios.standard")}
                </Chip>
              </div>
              <ol className={styles.steps}>
                {selected.scenario.steps.map((step, index) => (
                  <StepLine key={index} step={step} />
                ))}
              </ol>
            </>
          ) : (
            <span className={styles.muted}>{t("library.empty")}</span>
          )}
        </div>
        <div className={styles.list}>
          {games.map((game) => (
            <button
              key={game.name}
              type="button"
              className={`${styles.row} ${styles.plainRow} ${game.name === selected?.name ? styles.selected : ""}`}
              onClick={() => setPicked(game.name)}
            >
              <span className={styles.name}>
                <b>{game.name}</b>
              </span>
              <Chip>{t("library.steps", { count: game.scenario.steps.length })}</Chip>
            </button>
          ))}
        </div>
      </div>
    </Page>
  );
}
