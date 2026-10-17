import { useState } from "react";
import { useTranslation } from "react-i18next";
import { useNavigate } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";
import plans from "./PlansPage.module.css";

export function PlansPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const { lab, status } = useLab();
  const [error, setError] = useState<string | null>(null);
  const games = lab.data?.games ?? [];
  const sets = Object.entries(lab.data?.sets ?? {});
  const enabled = games.filter((game) => game.enabled).map((game) => game.name);
  const start = (set: string | null) => {
    setError(null);
    api
      .startRun(set, null)
      .then(() => {
        status.refresh();
        navigate("/run");
      })
      .catch((reason: unknown) => setError(errorText(reason)));
  };
  return (
    <Page title={t("nav.plans")}>
      {lab.error ? <Notice tone="error">{lab.error}</Notice> : null}
      {error ? <Notice tone="error">{error}</Notice> : null}
      <p className={styles.muted}>{t("plans.hint")}</p>
      <div className={plans.grid}>
        <PlanCard title={t("plans.enabled")} names={enabled} onStart={() => start(null)} />
        {sets.map(([set, names]) => (
          <PlanCard key={set} title={set} names={names ?? []} onStart={() => start(set)} />
        ))}
      </div>
    </Page>
  );
}

function PlanCard({ title, names, onStart }: { title: string; names: string[]; onStart: () => void }) {
  const { t } = useTranslation();
  return (
    <section className={styles.card}>
      <div className={styles.cardHead}>
        <h2>{title}</h2>
        <Chip>{t("plans.games", { count: names.length })}</Chip>
      </div>
      <ul className={plans.games}>
        {names.map((name) => (
          <li key={name}>{name}</li>
        ))}
      </ul>
      <div className={styles.actions}>
        <Button variant="primary" disabled={names.length === 0} onClick={onStart}>
          <Icon name="play" size={14} />
          {t("plans.start")}
        </Button>
      </div>
    </section>
  );
}
