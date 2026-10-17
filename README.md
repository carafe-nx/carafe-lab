# Carafe Lab

Carafe Lab checks [Carafe](https://github.com/carafe-nx/carafe) without a console: it builds games into NSPs the way
Carafe does, runs them in the [Eden](https://git.eden-emu.dev/eden-emu/eden) emulator in Docker, drives them with
button presses and writes a report with screenshots, milestones and logs.

It does not replace the console: the console stays the last check before a release. Lab catches what breaks between
releases — a game that no longer starts, a black screen, a hang, a save that is lost — and shows where it broke.

## Layout

- `app/` — Lab itself: Rust crates and, later, the window.
- `scenarios/` — what Lab does in a game, step by step.
- `patches/eden/` — Lab's changes to Eden, one patch per fix.
- `external/` — Eden and Carafe as git submodules pinned to exact commits; never edited.
- `docker/` — the images Eden is built and run in.
- `tools/` — probe programs, the gdb helper, page viewers.
- `docs/` — how Lab works, decisions, research.

## Getting started

[docs/lab.md](docs/lab.md) has the steps: submodules, Carafe's runtime, the Docker images, `lab.json`, a first run.

## License

GPL-3.0-or-later, as Carafe and Eden.
