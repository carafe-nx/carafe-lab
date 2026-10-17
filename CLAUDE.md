# Carafe Lab

Carafe Lab tests Carafe without a console: it packs games with Carafe's crates and runtime, runs them in the Eden
emulator in Docker and reports what happened. The plan is [decision 3](docs/decisions/0003-carafe-lab.md); how to set it
up is [docs/lab.md](docs/lab.md).

## Layout

- `app/` — Cargo workspace (`crates/*`), later npm workspaces with the window in `ui/`, as in Carafe's `app/`.
  - `lab-core` — pure: `lab.json`, scenarios, log milestones, picture comparison, settings sets, probe runs. No files,
    processes, Docker or clocks except through arguments; every rule has a unit test.
  - `lab-run` — a run: the steps of a scenario against the `Emulator` trait, and the report.
  - `lab-eden` — the `Emulator` for Eden in Docker.
  - `lab-cli` — the command line `carafe-lab` and the live page; the composition root.
- `scenarios/` — scenario files a game in `lab.json` names with `"scenario": "scenarios/…"`.
- `patches/eden/` — Lab's changes to Eden, applied in name order when Eden is built.
- `external/eden`, `external/carafe` — git submodules pinned to exact commits. They are never edited: Eden's changes
  go into `patches/eden/`, Carafe's into Carafe's own repository.
- `docker/` — `eden-build` (the build image and scripts), `mesa-d3d12` (Mesa for the PC's GPU), `eden` (the run image).
- `tools/` — probe programs, `eden-gdb.sh`, Playwright page viewers.
- `docs/` — decisions and research. Rules for keeping them are in `docs/README.md`.
- `build/` — everything built and every run; never committed.

## Building

- From `app/`: `cargo build --release`, `cargo test`, `cargo clippy --all-targets -- -D warnings`, `cargo fmt --check`.
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
