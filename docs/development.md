# Carafe Lab

Carafe Lab tests Carafe without a console: it packs games with Carafe's crates and runtime, runs them in the Eden
emulator in Docker and reports what happened. The plan is [decision 3](decisions/0003-carafe-lab.md); how to set it
up is [docs/lab.md](lab.md).

## Layout

- `app/` — Cargo workspace (`crates/*`) and npm workspace with the window in `ui/`, as in Carafe's `app/`.
  - `lab-core` — pure: `lab.json`, `lab.local.json`, scenarios, log milestones, picture comparison, settings sets,
    probe runs. No processes, Docker or clocks except through arguments; every rule has a unit test.
  - `lab-run` — a run: the steps of a scenario against the `Emulator` trait, the report, the history of runs and the
    queue for the emulator.
  - `lab-eden` — the `Emulator` for Eden in Docker.
  - `lab-cli` — the command line `carafe-lab`; the composition root of a run.
  - `lab-desktop` — the Tauri window. It reads the lab through the crates and starts runs as `carafe-lab run --json`
    in their own process, so a run outlives the window.
  - `ui/` — React in Carafe's style: `app/` (shell, routes), `pages/`, `features/lab/`, `shared/` (api with ts-rs
    bindings, i18n, theme, controls). Types come from Rust through ts-rs: `npm run bindings`.
- `patches/eden/`, `patches/mesa/` — Lab's changes to Eden and to Mesa, applied in name order when each is built.
- `external/eden`, `external/mesa`, `external/carafe` — git submodules pinned to exact commits (Mesa shallow, at its
  release tag). They are never edited: Eden's and Mesa's changes go into `patches/`, Carafe's into Carafe's own
  repository.
- `docker/` — `eden-build` (Eden's build image and scripts), `mesa-build` and `mesa-d3d12` (Mesa for the PC's GPU:
  its build image, its build script and the image of the built files), `eden` (the run image).
- `tools/` — probe programs, `eden-gdb.sh`, Playwright page viewers.
- `assets/` — static images, each stored once: `logo/` (the flask and the app icon, Carafe's), `github/` (the README
  banner).
- `docs/` — decisions and research. Rules for keeping them are in `docs/README.md`.
- `build/` — everything built and every run; never committed.

## Building

- From `app/`: `npm run dev` opens the window (it builds `carafe-lab` first); `npm run check` runs formatting, clippy,
  Rust tests, the TypeScript check and window outcome tests; `npm run cdp -- screenshot <file>` captures the window
  in development.
- Run Cargo tests from `app/` so Cargo loads `app/.cargo/config.toml` and exports TypeScript types to
  `ui/src/shared/api/bindings/`. Running Cargo from the parent directory with `--manifest-path` can instead leave
  duplicate generated files under individual crates. Only the UI bindings are versioned.
- Eden and its images build only in containers: `docker compose --profile build …` (see `docs/lab.md`). Git for
  Windows must not fetch Carafe's own submodules: it rewrites line endings.
- Build containers default to 2 CPUs, 2 jobs and 4 GB (`LAB_BUILD_*`); runs to 4 CPUs and 6 GB.

## Rules

The host page-table regression test links Eden's patched `src/common/page_table.cpp` with a small allocation
backend. It checks identity mappings for ordinary, debug and GPU-cached pages, traversal across page boundaries,
unmapped pages and addresses outside the table. After building Eden, run it without downloading dependencies:

```sh
docker compose --profile build run --rm --no-deps --pull never eden-build make -C /lab/tools/probes host-tests CXX=g++-14 OUT=/tmp/page-table-test
```

The same target checks OpenGL memory budget parsing, pressure arithmetic and retired-object lifetimes against
the patched Eden headers. The `memory` Windows probe in `tools/probes` separates address reservation from touched
physical pages. Renderer pressure policy and the meaning of each reported memory layer are defined in
[decision 13](decisions/0013-console-memory-and-host-renderer-pressure.md).

The target also checks the live-container single-file lookup using Python's standard library. The lookup reads
only the current title's user save, device save and SD logs, in that order. File checks avoid pulling the entire
NAND snapshot on each poll. Images without Python retain the snapshot lookup as a compatibility fallback.

The native OpenGL staging test compiles the patched `gl_staging_buffer_pool.cpp` and `gl_resource_manager.cpp`,
with stdout logging and fatal assertions, then exercises their real GL buffers, mappings and fences on the WSL
GPU. It checks borrowed mapping copies, final upload fences, deferred downloads, pressure reclamation, and data
integrity across queued copies. Run it after Eden's source has been patched by the normal build:

```sh
docker compose --profile build run --rm --no-deps --pull never opengl-staging-tests
```

`LAB_EDEN_IMAGE` selects the image containing Mesa for this test. It needs `/dev/dxg`; it mounts no game or console
files, runs without a network and uses the build limits. The test does not establish real-game stability.

It also checks repeated buffer expansion without presentation ticks, byte-budget reclamation, preservation of
copied data and rejection of a missing completion fence. Buffer retirement follows
[decision 14](decisions/0014-buffer-retirement-during-loading.md).

- Firmware, keys, games and NSPs never go into the repository; the keys file is mounted read-only and its path is cut
  out of every log and report.
- Text files use LF line endings; patches are stored byte for byte (`.gitattributes`).
- Commit messages follow Conventional Commits: one line in English, `type(scope): description`.
- A decision with a reason goes into `docs/decisions/` as a new numbered file, in English.
- ROADMAP.md: one line per item with a `bug`/`game`/`feature`/`research` tag. Planned items go in priority order, bold
  while in progress; done items get `[x]` and go to the top of Done.
- A diagnostic is a temporary patch only while it is needed; a lasting one becomes a named flag.
