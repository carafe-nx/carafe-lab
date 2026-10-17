# 5. The firmware is chosen in Lab's window

Date: 2026-10-17.

## Context

[Decision 3](0003-carafe-lab.md) wants an Eden that behaves like the Switch: 4 cores, 4 GB, docked, and "the firmware
chosen in Carafe's settings". Carafe has no firmware setting: it packs NSPs and never needs the firmware. Lab's Eden
ran without any firmware.

## Decision

- Eden's `qt-config.ini` names the console's settings outright: multi-core, the 4 GB memory layout, docked. The
  container already has 4 CPUs.
- The firmware is optional and chosen in the window's Emulator screen, a folder of `.nca` files kept in
  `lab.local.json` next to the console files. It never leaves the PC.
- Before Eden starts, Lab copies the folder's `.nca` files into the NAND volume's `system/Contents/registered/` and
  marks them with a stamp of the names and sizes; the copy is made again only when the folder changes, and removed
  when the choice is forgotten.
- Only `user/` and `system/save/` are copied back from the NAND to the PC, so the firmware does not travel with the
  save data after every Eden.

## Rejected

- A firmware setting in Carafe: Carafe would carry a choice it never uses.
- The firmware in the host's `nand/` folder: hundreds of megabytes copied into the volume and back with every Eden.
