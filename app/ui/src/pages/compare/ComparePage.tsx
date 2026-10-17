import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";
import { useParams } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { outcomeTone } from "@/features/lab/outcome";
import { folderName } from "@/features/lab/useRunEvents";
import type { GameSummary } from "@/shared/api/bindings/GameSummary";
import type { RunSummary } from "@/shared/api/bindings/RunSummary";
import { api } from "@/shared/api/commands";
import { Chip } from "@/shared/ui/Chip";
import { Notice } from "@/shared/ui/Notice";
import { Page } from "@/shared/ui/Page";
import styles from "@/shared/ui/lab.module.css";
import compare from "./ComparePage.module.css";

function gameFolder(game: GameSummary): string {
  return game.dir || game.name.replace(/[^A-Za-z0-9_-]/g, "_");
}

function stampDiffers(a: RunSummary, b: RunSummary): string[] {
  const keys = ["carafeCommit", "runtimeHash", "labCommit", "edenImage"] as const;
  return keys.filter((key) => a.stamp[key] !== b.stamp[key]);
}

export function ComparePage() {
  const { t } = useTranslation();
  const { a, b } = useParams();
  const { runs } = useLab();
  const first = runs.data?.find((run) => run.dir === a) ?? null;
  const second = runs.data?.find((run) => run.dir === b) ?? null;
  if (!first || !second) {
    return (
      <Page title={t("compare.title")}>
        <Notice tone="info">{t("compare.missing")}</Notice>
      </Page>
    );
  }
  const names = [...new Set([...first.games, ...second.games].map((game) => game.name))];
  const changed = stampDiffers(first, second);
  const unstamped = [first, second].some((run) => Object.values(run.stamp).every((value) => value === null));
  return (
    <Page title={t("compare.title")}>
      <div className={compare.heads}>
        <RunHead run={first} />
        <RunHead run={second} />
      </div>
      <p className={styles.muted}>
        {unstamped
          ? t("compare.noStamp")
          : changed.length === 0
          ? t("compare.sameParts")
          : t("compare.changedParts", { parts: changed.map((key) => t(`compare.part.${key}`)).join(", ") })}
      </p>
      {names.map((name) => (
        <GameCompare
          key={name}
          name={name}
          first={first}
          second={second}
          a={first.games.find((game) => game.name === name) ?? null}
          b={second.games.find((game) => game.name === name) ?? null}
        />
      ))}
    </Page>
  );
}

function RunHead({ run }: { run: RunSummary }) {
  return (
    <div className={styles.card}>
      <b>{run.started}</b>
      <span className={styles.mono}>
        {run.stamp.carafeCommit ?? "—"} · {run.stamp.runtimeHash ?? "—"}
      </span>
    </div>
  );
}

function GameCompare({
  name,
  first,
  second,
  a,
  b,
}: {
  name: string;
  first: RunSummary;
  second: RunSummary;
  a: GameSummary | null;
  b: GameSummary | null;
}) {
  const { t } = useTranslation();
  const [shotsA, setShotsA] = useState<string[]>([]);
  const [shotsB, setShotsB] = useState<string[]>([]);
  const folderA = a ? gameFolder(a) : null;
  const folderB = b ? gameFolder(b) : null;
  useEffect(() => {
    if (folderA) void api.runScreenshots(first.dir, folderA).then(setShotsA).catch(() => setShotsA([]));
    if (folderB) void api.runScreenshots(second.dir, folderB).then(setShotsB).catch(() => setShotsB([]));
  }, [folderA, folderB, first.dir, second.dir]);
  const shotNames = [...new Set([...shotsA, ...shotsB].map(folderName))];
  return (
    <section className={styles.card}>
      <div className={compare.gameHead}>
        <h2>{name}</h2>
        <Side game={a} />
        <Side game={b} />
      </div>
      {shotNames.length === 0 ? <span className={styles.muted}>{t("compare.noShots")}</span> : null}
      {shotNames.map((shot) => (
        <div key={shot} className={compare.shotRow}>
          <span className={compare.shotName}>{shot}</span>
          <Shot path={shotsA.find((path) => folderName(path) === shot) ?? null} />
          <Shot path={shotsB.find((path) => folderName(path) === shot) ?? null} />
        </div>
      ))}
    </section>
  );
}

function Side({ game }: { game: GameSummary | null }) {
  const { t } = useTranslation();
  if (!game) {
    return <span className={styles.muted}>{t("compare.notInRun")}</span>;
  }
  return (
    <span className={compare.side}>
      <Chip tone={outcomeTone(game.outcome)}>{t(`outcome.${game.outcome}`)}</Chip>
      {game.seconds > 0 ? <span className={styles.muted}>{t("units.seconds", { count: Math.round(game.seconds) })}</span> : null}
    </span>
  );
}

function Shot({ path }: { path: string | null }) {
  const { t } = useTranslation();
  const [url, setUrl] = useState<string | null>(null);
  useEffect(() => {
    setUrl(null);
    if (path) void api.readImage(path).then(setUrl).catch(() => setUrl(null));
  }, [path]);
  if (!path) {
    return <div className={compare.noShot}>{t("compare.noShot")}</div>;
  }
  return url ? <img className={compare.shot} src={url} alt="" /> : <div className={compare.noShot}>…</div>;
}
