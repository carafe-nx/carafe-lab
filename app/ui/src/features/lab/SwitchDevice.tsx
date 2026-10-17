import type { ReactNode } from "react";
import type { SwitchButton } from "@/shared/api/bindings/SwitchButton";
import styles from "./SwitchDevice.module.css";

type Direction = "n" | "s" | "w" | "e";

const DIRECTIONS: Record<Direction, [number, number]> = { n: [0, -1], s: [0, 1], w: [-1, 0], e: [1, 0] };

function arrow(x: number, y: number, dx: number, dy: number, size: number): string {
  const px = -dy;
  const py = dx;
  return [
    [x + dx * size, y + dy * size],
    [x - dx * size * 0.65 + px * size, y - dy * size * 0.65 + py * size],
    [x - dx * size * 0.65 - px * size, y - dy * size * 0.65 - py * size],
  ]
    .map((point) => point.join(","))
    .join(" ");
}

type PartProps = { button: SwitchButton | "HOME" | "CAPTURE"; lit: ReadonlySet<string>; children: ReactNode };

function Part({ button, lit, children }: PartProps) {
  return <g className={lit.has(button) ? styles.on : undefined}>{children}</g>;
}

type StickProps = {
  cx: number;
  cy: number;
  ids: Record<Direction, SwitchButton> & { c: SwitchButton };
  lit: ReadonlySet<string>;
};

function Stick({ cx, cy, ids, lit }: StickProps) {
  return (
    <>
      <Part button={ids.c} lit={lit}>
        <circle className={styles.ring} cx={cx} cy={cy} r={10} />
        <circle className={styles.key} cx={cx} cy={cy} r={6.6} />
      </Part>
      {(Object.keys(DIRECTIONS) as Direction[]).map((dir) => {
        const [dx, dy] = DIRECTIONS[dir];
        return (
          <Part key={dir} button={ids[dir]} lit={lit}>
            <polygon className={styles.key} points={arrow(cx + dx * 13, cy + dy * 13, dx, dy, 1.5)} />
          </Part>
        );
      })}
    </>
  );
}

type FourProps = {
  cx: number;
  cy: number;
  ids: Record<Direction, SwitchButton>;
  letters: boolean;
  lit: ReadonlySet<string>;
};

function Four({ cx, cy, ids, letters, lit }: FourProps) {
  return (
    <>
      {(Object.keys(DIRECTIONS) as Direction[]).map((dir) => {
        const [dx, dy] = DIRECTIONS[dir];
        const x = cx + dx * 9.2;
        const y = cy + dy * 9.2;
        return (
          <Part key={dir} button={ids[dir]} lit={lit}>
            <circle className={styles.key} cx={x} cy={y} r={4.2} />
            {letters ? (
              <text className={styles.keyText} x={x} y={y + 1.3}>
                {ids[dir]}
              </text>
            ) : (
              <polygon className={styles.keyText} points={arrow(x, y, dx, dy, 1.4)} />
            )}
          </Part>
        );
      })}
    </>
  );
}

type SwitchDeviceProps = {
  lit: SwitchButton[];
  children: ReactNode;
};

export function SwitchDevice({ lit, children }: SwitchDeviceProps) {
  const on = new Set<string>(lit);
  return (
    <div className={styles.device}>
      <svg viewBox="-6 -16 251 124" aria-hidden="true">
        <Part button="ZL" lit={on}>
          <rect className={styles.key} x={9} y={-11.5} width={24.5} height={7.5} rx={3.75} />
          <text className={styles.keyText} x={21.25} y={-6.5}>
            ZL
          </text>
        </Part>
        <Part button="ZR" lit={on}>
          <rect className={styles.key} x={205.5} y={-11.5} width={24.5} height={7.5} rx={3.75} />
          <text className={styles.keyText} x={217.75} y={-6.5}>
            ZR
          </text>
        </Part>
        <Part button="L" lit={on}>
          <path className={styles.key} d="M4.6 5.8 A16.5 16.5 0 0 1 16.4 -2 H33.6 V-0.6 H16.6 A15 15 0 0 0 6 4.6 Z" />
        </Part>
        <Part button="R" lit={on}>
          <path
            className={styles.key}
            d="M234.4 5.8 A16.5 16.5 0 0 0 222.6 -2 H205.4 V-0.6 H222.4 A15 15 0 0 1 233 4.6 Z"
          />
        </Part>
        <path className={styles.shell} d="M35.9 0 H16 A16 16 0 0 0 0 16 V86 A16 16 0 0 0 16 102 H35.9 Z" />
        <path className={styles.shell} d="M203.1 0 H223 A16 16 0 0 1 239 16 V86 A16 16 0 0 1 223 102 H203.1 Z" />
        <rect className={styles.shell} x={35.9} y={0} width={167.2} height={102} />
        <Part button="MINUS" lit={on}>
          <rect className={styles.key} x={25.8} y={7.6} width={6.4} height={1.9} rx={0.95} />
        </Part>
        <Part button="PLUS" lit={on}>
          <path
            className={styles.key}
            d="M209.05 6.3 a0.95 0.95 0 0 1 1.9 0 V7.6 H212.25 a0.95 0.95 0 0 1 0 1.9 H210.95 V10.8 a0.95 0.95 0 0 1 -1.9 0 V9.5 H207.75 a0.95 0.95 0 0 1 0 -1.9 H209.05 Z"
          />
        </Part>
        <Stick cx={17.5} cy={28} ids={{ n: "LUP", s: "LDOWN", w: "LLEFT", e: "LRIGHT", c: "STICKL" }} lit={on} />
        <Stick cx={221.5} cy={60} ids={{ n: "RUP", s: "RDOWN", w: "RLEFT", e: "RRIGHT", c: "STICKR" }} lit={on} />
        <Four cx={17.5} cy={60} ids={{ n: "UP", s: "DOWN", w: "LEFT", e: "RIGHT" }} letters={false} lit={on} />
        <Four cx={221.5} cy={28} ids={{ n: "X", s: "B", w: "Y", e: "A" }} letters lit={on} />
        <Part button="CAPTURE" lit={on}>
          <rect className={styles.fixed} x={22.6} y={78.4} width={5.6} height={5.6} rx={1.2} />
        </Part>
        <Part button="HOME" lit={on}>
          <circle className={styles.fixed} cx={213.6} cy={81.2} r={3.3} />
        </Part>
      </svg>
      <div className={styles.screen}>{children}</div>
    </div>
  );
}
