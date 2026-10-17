import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";
import styles from "./LiveScreen.module.css";

const LIBRARY = navigator.userAgent.includes("Windows")
  ? "http://novnc.localhost/core/rfb.js"
  : "novnc://localhost/core/rfb.js";
const SERVER = "ws://127.0.0.1:6080/websockify";
const RETRY_MS = 1000;

type Rfb = EventTarget & {
  viewOnly: boolean;
  scaleViewport: boolean;
  resizeSession: boolean;
  showDotCursor: boolean;
  background: string;
  focus: () => void;
  disconnect: () => void;
};

type RfbClass = new (target: HTMLElement, url: string) => Rfb;

let library: Promise<RfbClass> | null = null;

function loadLibrary(): Promise<RfbClass> {
  library ??= import(/* @vite-ignore */ LIBRARY).then((module: { default: RfbClass }) => module.default);
  return library;
}

type LiveScreenProps = {
  control: boolean;
  title: string;
};

export function LiveScreen({ control, title }: LiveScreenProps) {
  const { t } = useTranslation();
  const holder = useRef<HTMLDivElement>(null);
  const client = useRef<Rfb | null>(null);
  const [connected, setConnected] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const controlRef = useRef(control);

  useEffect(() => {
    let stopped = false;
    let retry: number | undefined;
    const connect = async () => {
      const target = holder.current;
      if (!target || stopped) {
        return;
      }
      const RFB = await loadLibrary();
      if (stopped) {
        return;
      }
      target.replaceChildren();
      const rfb = new RFB(target, SERVER);
      rfb.viewOnly = !controlRef.current;
      rfb.scaleViewport = true;
      rfb.resizeSession = false;
      rfb.showDotCursor = false;
      rfb.background = "#000";
      rfb.addEventListener("connect", () => {
        setConnected(true);
        setError(null);
      });
      rfb.addEventListener("disconnect", () => {
        setConnected(false);
        if (!stopped) {
          retry = window.setTimeout(() => void connect(), RETRY_MS);
        }
      });
      client.current = rfb;
    };
    connect().catch((reason: unknown) => setError(String(reason)));
    return () => {
      stopped = true;
      window.clearTimeout(retry);
      client.current?.disconnect();
      client.current = null;
    };
  }, []);

  useEffect(() => {
    controlRef.current = control;
    const rfb = client.current;
    if (rfb) {
      rfb.viewOnly = !control;
      if (control) {
        rfb.focus();
      }
    }
  }, [control]);

  return (
    <div className={styles.live} title={title}>
      <div ref={holder} className={styles.canvas} />
      {error ? (
        <span className={styles.state}>{t("run.liveFailed", { error })}</span>
      ) : connected ? null : (
        <span className={styles.state}>{t("run.liveConnecting")}</span>
      )}
    </div>
  );
}
