# Carafe Lab

Carafe Lab tests Carafe without a console: it packs games with Carafe's crates and runtime, runs them in the Eden
emulator in Docker and reports what happened. The plan is [decision 3](docs/decisions/0003-carafe-lab.md); how to set it
up is [docs/lab.md](docs/lab.md).

## Layout

- `app/` — Cargo workspace (`crates/*`) and npm workspace with the window in `ui/`, as in Carafe's `app/`.
  - `lab-core` — pure: `lab.json`, `lab.local.json`, scenarios, log milestones, picture comparison, settings sets,
    probe runs. No processes, Docker or clocks except through arguments; every rule has a unit test.
  - `lab-run` — a run: the steps of a scenario against the `Emulator` trait, the report, the history of runs and the
    queue for the emulator.
  - `lab-eden` — the `Emulator` for Eden in Docker.
  - `lab-cli` — the command line `carafe-lab` and the live page; the composition root of a run.
  - `lab-desktop` — the Tauri window. It reads the lab through the crates and starts runs as `carafe-lab run --json`
    in their own process, so a run outlives the window.
  - `ui/` — React in Carafe's style: `app/` (shell, routes), `pages/`, `features/lab/`, `shared/` (api with ts-rs
    bindings, i18n, theme, controls). Types come from Rust through ts-rs: `npm run bindings`.
- `scenarios/` — scenario files a game in `lab.json` names with `"scenario": "scenarios/…"`; like `lab.json` they are
  each developer's own, and git ignores them.
- `patches/eden/`, `patches/mesa/` — Lab's changes to Eden and to Mesa, applied in name order when each is built.
- `external/eden`, `external/mesa`, `external/carafe` — git submodules pinned to exact commits (Mesa shallow, at its
  release tag). They are never edited: Eden's and Mesa's changes go into `patches/`, Carafe's into Carafe's own
  repository.
- `docker/` — `eden-build` (Eden's build image and scripts), `mesa-build` and `mesa-d3d12` (Mesa for the PC's GPU:
  its build image, its build script and the image of the built files), `eden` (the run image).
- `tools/` — probe programs, `eden-gdb.sh`, Playwright page viewers.
- `docs/` — decisions and research. Rules for keeping them are in `docs/README.md`.
- `build/` — everything built and every run; never committed.

## Building

- From `app/`: `npm run dev` opens the window (it builds `carafe-lab` first); `npm run check` runs formatting, clippy,
  Rust tests and the TypeScript check; `npm run cdp -- screenshot <file>` captures the window in development.
- Eden and its images build only in containers: `docker compose --profile build …` (see `docs/lab.md`). Git for
  Windows must not fetch Carafe's own submodules: it rewrites line endings.
- Build containers default to 2 CPUs, 2 jobs and 4 GB (`LAB_BUILD_*`); runs to 4 CPUs and 6 GB.

## Rules

- Firmware, keys, games and NSPs never go into the repository; the keys file is mounted read-only and its path is cut
  out of every log and report.
- Text files use LF line endings; patches are stored byte for byte (`.gitattributes`).
- Commit messages follow Conventional Commits: one line in English, `type(scope): description`.
- A decision with a reason goes into `docs/decisions/` as a new numbered file, in English.
- ROADMAP.md: one line per item with a `bug`/`game`/`feature`/`research` tag. Planned items go in priority order, bold
  while in progress; done items get `[x]` and go to the top of Done.
- A diagnostic is a temporary patch only while it is needed; a lasting one becomes a named flag.
