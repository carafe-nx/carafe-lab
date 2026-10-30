# Carafe Lab documentation

- [lab.md](lab.md) — setting Lab up, `lab.json`, scenarios and their steps, the report.
- [decisions/](decisions/) — decisions in force: what was chosen, why, and what was rejected.
- [research/automated-testing/REPORT.md](research/automated-testing/REPORT.md) — why an emulator and which one.
- [research/cloud-reports/](research/cloud-reports/) — the material of the cloud sessions that brought the bench up.
- [research/lab-speed/REPORT.md](research/lab-speed/REPORT.md) — why the lab loads and runs games slower than the
  console: the emulated CPU, the lab's own time and what can be done.
- [research/lab-nce/ANALYSIS.md](research/lab-nce/ANALYSIS.md) — what Eden's NCE needs to run Carafe's guest code
  natively, what it would still cost, and the plan.
- [research/lab-gpu/ANALYSIS.md](research/lab-gpu/ANALYSIS.md) — where a frame waits on the GPU path once NCE runs the
  guest's code: synchronous flushes, a present that waits for the GPU, and what to change.
- [research/eden-stop-crash.md](research/eden-stop-crash.md) — Eden's crash in Mesa's d3d12 queries when software
  stops on the WSL GPU.

## Keeping it

- A new decision is a new file in `decisions/`, numbered in order: context, decision, rejected options. A decision that
  replaces an older one says so, and the older file gets a line pointing to the new one.
- A fact about third-party code names the file, the lines and the commit it was read at; anything not checked is marked
  "not verified".
- Plans and their status live in [ROADMAP.md](../ROADMAP.md), not here.
- Everything is in English.
