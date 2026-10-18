# 7. Eden runs handheld, at 1280×720

Date: 2026-10-18.

## Context

[Decision 3](0003-carafe-lab.md) and [decision 5](0005-firmware-chosen-in-lab.md) set Eden up docked. Docked, the
Switch gives games a 1920×1080 screen, so every screenshot and every check worked on 1080p pictures, while Carafe's
games are meant to be checked at 720p, the Switch's own screen. Eden's resolution scale was already 1× (`Res1X`), so
the 1080p came from the docked mode alone.

## Decision

- Eden runs handheld (`use_docked_mode=0`): games get the 1280×720 screen, as on the console in the hands.
- The resolution scale stays 1×, so screenshots are the game's own pixels, not an upscale.
- The rest of the console setup stays: 4 cores, the 4 GB memory layout.

## Rejected

- Docked with the resolution scale at ¾ (`Res3_4X`): the picture would be 1440×810, not 720p, and games would still
  see a docked console.
