# Carafe Lab documentation

- [lab.md](lab.md) — setting Lab up, `lab.json`, scenarios and their steps, the report.
- [decisions/](decisions/) — decisions in force: what was chosen, why, and what was rejected.
- [research/automated-testing/REPORT.md](research/automated-testing/REPORT.md) — why an emulator and which one.
- [research/cloud-reports/](research/cloud-reports/) — the material of the cloud sessions that brought the bench up.
- [research/eden-stop-crash.md](research/eden-stop-crash.md) — Eden's crash in Mesa's d3d12 queries when software
  stops on the WSL GPU.

## Keeping it

- A new decision is a new file in `decisions/`, numbered in order: context, decision, rejected options. A decision that
  replaces an older one says so, and the older file gets a line pointing to the new one.
- A fact about third-party code names the file, the lines and the commit it was read at; anything not checked is marked
  "not verified".
- Plans and their status live in [ROADMAP.md](../ROADMAP.md), not here.
- Everything is in English.
