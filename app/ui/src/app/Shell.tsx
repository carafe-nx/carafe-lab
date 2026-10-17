import { useTranslation } from "react-i18next";
import { NavLink, Outlet } from "react-router";
import { useLab } from "@/features/lab/LabProvider";
import { CarafeMark } from "@/shared/ui/CarafeMark";
import { Chip } from "@/shared/ui/Chip";
import { Icon, type IconName } from "@/shared/ui/Icon";
import { Notice } from "@/shared/ui/Notice";
import { Button } from "@/shared/ui/Button";
import styles from "./Shell.module.css";

const main: { to: string; icon: IconName; label: string }[] = [
  { to: "/library", icon: "grid", label: "nav.library" },
  { to: "/plans", icon: "list", label: "nav.plans" },
  { to: "/run", icon: "play", label: "nav.run" },
  { to: "/history", icon: "clock", label: "nav.history" },
  { to: "/versions", icon: "rebuild", label: "nav.versions" },
];

const tools: { to: string; icon: IconName; label: string }[] = [
  { to: "/emulator", icon: "monitor", label: "nav.emulator" },
  { to: "/scenarios", icon: "flow", label: "nav.scenarios" },
];

export function Shell() {
  const { t } = useTranslation();
  const { exit, dismissExit } = useLab();
  return (
    <div className={styles.window}>
      <TitleBar />
      <nav className={styles.nav} aria-label={t("nav.label")}>
        {main.map((item) => (
          <NavItem key={item.to} {...item} />
        ))}
        <span className={styles.separator} />
        {tools.map((item) => (
          <NavItem key={item.to} {...item} />
        ))}
      </nav>
      <main className={styles.body}>
        {exit ? (
          <div className={styles.exit}>
            <Notice tone="error">
              <b>{t("run.exited", { code: exit.code ?? "—" })}</b>
              <pre className={styles.output}>{exit.output}</pre>
              <Button variant="ghost" onClick={dismissExit}>
                {t("common.dismiss")}
              </Button>
            </Notice>
          </div>
        ) : null}
        <Outlet />
      </main>
    </div>
  );
}

function NavItem({ to, icon, label }:{ to: string; icon: IconName; label: string }) {
  const { t } = useTranslation();
  return (
    <NavLink to={to} className={({ isActive }) => (isActive ? `${styles.link} ${styles.active}` : styles.link)}>
      <Icon name={icon} size={16} />
      {t(label)}
    </NavLink>
  );
}

function TitleBar() {
  const { t } = useTranslation();
  const { status } = useLab();
  const data = status.data;
  const holder = data?.queue.holder ?? null;
  const waiting = data?.queue.waiting.length ?? 0;
  let chip = <Chip>{t("emulator.checking")}</Chip>;
  if (data) {
    if (holder) {
      chip = <Chip tone="acc">{t("emulator.busy", { who: t(`who.${holder.who}`, { defaultValue: holder.who }), what: holder.what })}</Chip>;
    } else if (data.docker === "ready") {
      chip = <Chip tone="ok">{t("emulator.free")}</Chip>;
    } else {
      chip = <Chip tone="warn">{t(`docker.${data.docker}`)}</Chip>;
    }
  }
  return (
    <header className={styles.titlebar}>
      <CarafeMark size={16} />
      <b>Carafe Lab</b>
      {chip}
      {waiting > 0 ? <Chip>{t("emulator.waiting", { count: waiting })}</Chip> : null}
      <span className={styles.right}>
        {data?.problem ? <Chip tone="bad">{t("lab.broken")}</Chip> : null}
        {data?.disk.low ? (
          <span title={t("lab.lowSpaceHint")}>
            <Chip tone="warn">
              {t("lab.lowSpace", {
                gb: Math.min(...[data.disk.workFreeGb, data.disk.tempFreeGb].filter((gb) => gb !== null)),
              })}
            </Chip>
          </span>
        ) : null}
        {data && data.keys.complete !== true ? (
          <NavLink to="/emulator" className={styles.chipLink}>
            <Chip tone="warn">{t("lab.noKeys")}</Chip>
          </NavLink>
        ) : null}
        <span>Carafe · {data?.carafeCommit ?? "—"}</span>
      </span>
    </header>
  );
}
