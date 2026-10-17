# 4. Mesa comes from a submodule and patches, like Eden

Date: 2026-10-17.

## Context

[Decision 2](0002-wsl-gpu-patched-mesa.md) runs Eden on the PC's GPU through Mesa's d3d12 driver with one patch of ours.
Lab's Mesa image was copied from the bench, which had built it from Ubuntu's source package downloaded inside a
Docker build stage. That source was not in the repository, so a second fix could not be written against it or
checked, and every change rebuilt Mesa from nothing.

The second fix was needed: Eden crashes in `end_subquery` whenever software stops on the WSL GPU
([research](../research/eden-stop-crash.md)), so the one Eden of [decision 3](0003-carafe-lab.md) was replaced after
every game.

## Decision

- `external/mesa` is a shallow git submodule at the tag `mesa-25.2.8` of `gitlab.freedesktop.org/mesa/mesa`.
- Lab's changes are numbered files in `patches/mesa/`, applied in name order, as for Eden.
- `docker compose --profile build run --rm mesa-build` builds only the d3d12 driver in the volume
  `carafe-lab-mesa-src`, so a new patch rebuilds only what it touches, and installs into `build/mesa-d3d12/`; the image
  `carafe-lab-mesa-d3d12` is made of those files.
- Ubuntu's own patches to Mesa are no longer applied: the upstream tag and our patches are the whole source.

## Rejected

- Ubuntu's source package downloaded during the image build: not in the repository, not patchable by file, rebuilt
  from nothing every time.
- Mesa's full history as the submodule: hundreds of megabytes for nothing the build needs.
