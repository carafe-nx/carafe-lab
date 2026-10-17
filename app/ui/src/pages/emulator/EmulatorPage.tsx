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

type Stage = {
  key: string;
  ok: boolean;
  off?: boolean;
  detail: string;
  hint?: string;
  fix: string | null;
  action?: ReactNode;
};

function firmwareStage(status: LabStatus, t: Translate, action: ReactNode): Stage {
  const firmware = status.firmware;
  const chosen = firmware.folderName !== null;
  const usable = chosen && firmware.problem === null;
  return {
    key: "firmware",
    ok: firmware.problem === null,
    off: !chosen && firmware.problem === null,
    detail:
      firmware.problem ?? (usable ? t("stages.firmwareReady", { folder: firmware.folderName, count: firmware.files }) : t("stages.firmwareNone")),
    hint: usable ? undefined : t("stages.firmwareHint"),
    fix: null,
    action,
  };
}

function keysDetail(status: LabStatus, t: Translate): string {
  const keys = status.keys;
  if (!keys.source) return t("stages.keysMissing");
  const where = t(`stages.keysFrom.${keys.source}`);
  if (keys.complete === null) return t("stages.keysUnreadable", { where });
  if (!keys.complete) return t("stages.keysIncomplete", { where });
  return t("stages.keysReady", { where, file: keys.fileName ?? "" });
}

function stages(status: LabStatus, t: Translate, keysAction: ReactNode, firmwareAction: ReactNode): Stage[] {
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
    firmwareStage(status, t, firmwareAction),
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
  const chooseFirmware = async () => {
    setError(null);
    const picked = await open({ multiple: false, directory: true, title: t("stages.firmwarePick") });
    if (typeof picked !== "string") return;
    try {
      await api.chooseFirmware(picked);
      status.refresh();
    } catch (reason) {
      setError(errorText(reason));
    }
  };
  const forgetFirmware = () => {
    setError(null);
    api
      .forgetFirmware()
      .then(() => status.refresh())
      .catch((reason: unknown) => setError(errorText(reason)));
  };
  const firmwareAction = (
    <span className={styles.actions}>
      <Button onClick={() => void chooseFirmware()}>
        <Icon name="folder" size={14} />
        {t("stages.firmwareChoose")}
      </Button>
      {data?.firmware.folderName ? (
        <Button variant="ghost" onClick={forgetFirmware}>
          {t("stages.firmwareForget")}
        </Button>
      ) : null}
    </span>
  );
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
            {stages(data, t, keysAction, firmwareAction).map((stage) => (
              <div key={stage.key} className={emulator.stage}>
                <span className={`${emulator.mark} ${stage.off ? emulator.off : stage.ok ? emulator.ok : emulator.bad}`}>
                  {stage.off ? "–" : stage.ok ? "✓" : "!"}
                </span>
                <span className={emulator.text}>
                  <b>{t(`stages.${stage.key}`)}</b>
                  <small>{stage.detail}</small>
                  {stage.hint ? <small>{stage.hint}</small> : null}
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
