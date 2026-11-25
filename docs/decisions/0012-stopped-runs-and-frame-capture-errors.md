# Stopped runs and frame capture errors do not pass

## Context

Both report and history counters counted `Outcome::Skip` as a pass. A manually stopped scenario could therefore
retain its skipped outcome while the run summary reported a successful game.

The Eden adapter discarded control-channel screenshot errors and tried a desktop screenshot instead. It also
accepted a frame file after its size stayed unchanged for one poll, even if the writer had not finished the PNG.
The ordinary screenshot step treated an unreadable image as a successful nonblank screenshot.

## Decision

Keep the existing outcome types and four result counters. Skipped games remain visible in the report but do not
increment any result counter. The window uses the same counting rules and shows skipped outcomes in a neutral
colour. A cancellation is neither a passed game nor a failed game.

For an active control-channel session, propagate frame capture errors. Desktop capture remains available when
there is no active session or emulation is paused. A frame is ready only when the PNG decoder accepts it,
including its end chunk. While waiting for the frame, preserve the emulator's termination reason if it exits.
The existing capture deadline remains unchanged.

A screenshot step fails if its file cannot be read or decoded. A valid flat-colour image retains its warning
outcome. Successful capture still establishes only that a picture was saved; game cards define gameplay checks.

## Rejected alternatives

Counting skipped games as failures would misclassify a user's cancellation. Keeping a fixed file-size delay or
increasing it would not establish that an image is complete. Silently substituting a desktop picture would lose
the active session's capture error.

## Validation

Regression tests exercise stopped scenarios in report, history and window counts, damaged screenshot files,
and PNG data without its end chunk. These cases fail before the corresponding fixes. A separate runner test
checks that a screenshot error retains the emulator's crash reason.
Adapter changes are covered by compilation and the shared PNG decoder tests; a live emulator capture during
a native crash has not yet been repeated.
