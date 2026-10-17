import { open } from "@tauri-apps/plugin-dialog";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import type { NspInfo } from "@/shared/api/bindings/NspInfo";
import { api, errorText } from "@/shared/api/commands";
import { Button } from "@/shared/ui/Button";
import { Chip } from "@/shared/ui/Chip";
import { Icon } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import styles from "@/shared/ui/lab.module.css";

type ReadyPlanProps = {
  onStart: (nsp: string, rebuild: boolean) => void;
};

export function ReadyPlan({ onStart }: ReadyPlanProps) {
  const { t } = useTranslation();
  const [info, setInfo] = useState<NspInfo | null>(null);
  const [error, setError] = useState<string | null>(null);
  const pick = async () => {
    const picked = await open({
      multiple: false,
      directory: false,
      title: t("ready.pick"),
      filters: [{ name: "NSP", extensions: ["nsp"] }],
    });
    if (typeof picked !== "string") {
      return;
    }
    setError(null);
    api
      .readNsp(picked)
      .then(setInfo)
      .catch((reason: unknown) => {
        setInfo(null);
        setError(errorText(reason));
      });
  };
  const debug = [info?.verbose ? t("ready.verbose") : null, info?.profile ? t("ready.profile") : null].filter(Boolean);
  return (
    <section className={styles.card}>
      <div className={styles.cardHead}>
        <h2>{t("ready.title")}</h2>
        {info ? <Chip tone="acc">{t("ready.runtime", { version: info.runtimeVersion })}</Chip> : null}
      </div>
      <p className={styles.muted}>{t("ready.hint")}</p>
      {error ? <Notice tone="error">{error}</Notice> : null}
      {info ? (
        <dl className={styles.kv}>
          <dt>{t("ready.game")}</dt>
          <dd>
            {info.game ?? info.title} · {t("ready.build", { number: info.buildNumber })}
          </dd>
          <dt>{t("ready.titleId")}</dt>
          <dd className={styles.mono}>{info.titleId}</dd>
          <dt>{t("ready.settings")}</dt>
          <dd>{[info.settings, ...debug].join(" · ")}</dd>
          <dt>{t("ready.folder")}</dt>
          <dd className={styles.mono}>{info.folderFound ? info.folder : t("ready.folderGone", { folder: info.folder })}</dd>
        </dl>
      ) : null}
      <div className={styles.actions}>
        <Button onClick={() => void pick()}>
          <Icon name="folder" size={14} />
          {t(info ? "ready.another" : "ready.pick")}
        </Button>
        <Button variant="primary" disabled={!info} onClick={() => info && onStart(info.path, false)}>
          <Icon name="play" size={14} />
          {t("ready.asIs")}
        </Button>
        <Button
          disabled={!info?.folderFound}
          title={info && !info.folderFound ? t("ready.rebuildNoFolder") : t("ready.rebuildHint")}
          onClick={() => info && onStart(info.path, true)}
        >
          <Icon name="rebuild" size={14} />
          {t("ready.rebuild")}
        </Button>
      </div>
    </section>
  );
}
