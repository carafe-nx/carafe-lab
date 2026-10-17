import type { SwitchButton } from "@/shared/api/bindings/SwitchButton";
import styles from "./JoyCon.module.css";

type Side = "left" | "right";

const SHOULDER: Record<Side, SwitchButton[]> = { left: ["ZL", "L"], right: ["ZR", "R"] };
const MENU: Record<Side, SwitchButton> = { left: "MINUS", right: "PLUS" };
const STICK: Record<Side, SwitchButton[]> = {
  left: ["LUP", "LDOWN", "LLEFT", "LRIGHT", "STICKL"],
  right: ["RUP", "RDOWN", "RLEFT", "RRIGHT", "STICKR"],
};
const FACE: Record<Side, (SwitchButton | null)[]> = {
  left: [null, "UP", null, "LEFT", null, "RIGHT", null, "DOWN", null],
  right: [null, "X", null, "Y", null, "A", null, "B", null],
};

export function JoyCon({ side, lit }: { side: Side; lit: SwitchButton[] }) {
  const on = (button: SwitchButton) => lit.includes(button);
  const stickOn = STICK[side].some(on);
  const face = (
    <span className={styles.face}>
      {FACE[side].map((button, index) =>
        button ? (
          <i key={button} className={on(button) ? styles.lit : undefined} title={button}>
            {side === "right" ? button : ""}
          </i>
        ) : (
          <i key={`gap-${index}`} className={styles.gap} />
        ),
      )}
    </span>
  );
  const stick = <span className={stickOn ? `${styles.stick} ${styles.lit}` : styles.stick} />;
  return (
    <span className={`${styles.joycon} ${styles[side]}`}>
      <span className={styles.shoulders}>
        {SHOULDER[side].map((button) => (
          <i key={button} className={on(button) ? styles.lit : undefined} title={button} />
        ))}
      </span>
      <i className={on(MENU[side]) ? `${styles.menu} ${styles.lit}` : styles.menu} title={MENU[side]} />
      {side === "left" ? stick : face}
      {side === "left" ? face : stick}
    </span>
  );
}
