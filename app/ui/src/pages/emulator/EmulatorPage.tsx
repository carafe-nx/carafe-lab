import { open } from "@tauri-apps/plugin-dialog";
import { type ReactNode, useState } from "react";
import { useTranslation } from "react-i18next";
import { useLab } from "@/features/lab/LabProvider";
import type { LabStatus } from "@/shared/api/bindings/LabStatus";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import { Spinner } from "@/shared/ui/Spinner";
import styles from "@/shared/ui/lab.module.css";
import emulator from "./EmulatorPage.module.css";

type Translate = (key: string, options?: Record<string, unknown>) => string;

type Stage = { key: string; ok: boolean; detail: string; fix: string | null; action?: ReactNode };

function keysDetail(status: LabStatus, t: Translate): string {
  const keys = status.keys;
  if (!keys.source) return t("stages.keysMissing");
  const where = t(`stages.keysFrom.${keys.source}`);
  if (keys.complete === null) return t("stages.keysUnreadable", { where });
  if (!keys.complete) return t("stages.keysIncomplete", { where });
  return t("stages.keysReady", { where, file: keys.fileName ?? "" });
}

function stages(status: LabStatus, t: Translate, keysAction: ReactNode): Stage[] {
  const dockerUp = status.docker === "ready" || status.docker === "noImage";
  const keysOk = status.keys.complete === true;
  return [
    {
      key: "lab",
      ok: status.problem === null,
      detail: status.problem ?? status.config,
      fix: status.problem ? t("stages.labFix") : null,
    },
    {
      key: "keys",
      ok: keysOk,
      detail: status.keys.problem ?? keysDetail(status, t),
      fix: keysOk ? null : t("stages.keysFix"),
      action: keysAction,
    },
    {
      key: "carafe",
      ok: status.carafeCommit !== null,
      detail: `${status.carafe} · ${status.carafeCommit ?? "—"}`,
      fix: status.carafeCommit ? null : t("stages.carafeFix"),
    },
    {
      key: "runtime",
      ok: status.runtimeBuilt,
      detail: status.runtimeBuilt ? t("stages.runtimeReady") : t("stages.runtimeMissing"),
      fix: status.runtimeBuilt ? null : t("stages.runtimeFix"),
    },
    {
      key: "docker",
      ok: dockerUp,
      detail: dockerUp ? t("stages.dockerRunning") : t(`docker.${status.docker}`),
      fix: dockerUp ? null : t("stages.dockerFix"),
    },
    {
      key: "image",
      ok: status.docker === "ready",
      detail: status.image,
      fix: status.docker === "noImage" ? t("stages.imageFix") : null,
    },
  ];
}

export function EmulatorPage() {
  const { t } = useTranslation();
  const { status } = useLab();
  const [error, setError] = useState<string | null>(null);
  const data = status.data;
  const choose = async () => {
    setError(null);
    const picked = await open({ multiple: false, directory: false, title: t("stages.keysPick") });
    if (typeof picked !== "string") return;
    try {
      await api.chooseKeys(picked);
      status.refresh();
    } catch (reason) {
      setError(errorText(reason));
    }
  };
  const forget = () => {
    setError(null);
    api
      .forgetKeys()
      .then(() => status.refresh())
      .catch((reason: unknown) => setError(errorText(reason)));
  };
  const keysAction = (
    <span className={styles.actions}>
      <Button onClick={() => void choose()}>
        <Icon name="key" size={14} />
        {t("stages.keysChoose")}
      </Button>
      {data?.keys.source === "lab" ? (
        <Button variant="ghost" onClick={forget}>
          {t("stages.keysForget")}
        </Button>
      ) : null}
    </span>
  );
  return (
    <Page title={t("nav.emulator")}>
      {status.error ? <Notice tone="error">{status.error}</Notice> : null}
      {error ? <Notice tone="error">{error}</Notice> : null}
      {!data ? <Spinner size="large" /> : null}
      {data ? (
        <div className={styles.split}>
          <div className={emulator.stages}>
            {stages(data, t, keysAction).map((stage) => (
              <div key={stage.key} className={emulator.stage}>
                <span className={stage.ok ? `${emulator.mark} ${emulator.ok}` : `${emulator.mark} ${emulator.bad}`}>{stage.ok ? "✓" : "!"}</span>
                <span className={emulator.text}>
                  <b>{t(`stages.${stage.key}`)}</b>
                  <small>{stage.detail}</small>
                  {stage.fix ? <code className={emulator.fix}>{stage.fix}</code> : null}
                  {stage.action ?? null}
                </span>
              </div>
            ))}
          </div>
          <aside className={styles.card}>
            <span className={styles.label}>{t("run.queue")}</span>
            {data.queue.holder ? (
              <Chip tone="acc">{t("emulator.busy", { who: t(`who.${data.queue.holder.who}`, { defaultValue: data.queue.holder.who }), what: data.queue.holder.what })}</Chip>
            ) : (
              <Chip tone="ok">{t("emulator.free")}</Chip>
            )}
            {data.queue.waiting.map((entry) => (
              <span key={entry.id} className={styles.muted}>
                {t("emulator.queued", { who: t(`who.${entry.who}`, { defaultValue: entry.who }), what: entry.what })}
              </span>
            ))}
            <span className={styles.label}>{t("emulator.work")}</span>
            <span className={styles.mono}>{data.work || "—"}</span>
          </aside>
        </div>
      ) : null}
    </Page>
  );
}
