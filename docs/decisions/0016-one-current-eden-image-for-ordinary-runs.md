# One current Eden image for ordinary runs

## Context

The single-emulator contract in decision 3 allowed a hidden contradiction: `edenImage` in `lab.json`
overrode the image built by Compose. CLI execution, desktop readiness and noVNC extraction all honored it.
A diagnostic image could therefore remain selected after a newer canonical image was built.

Image provenance used `docker image ls --quiet`, whose result could contain repeated IDs. Retention tagged
the mutable image name rather than the identity recorded for the run. Session reuse checked configuration
and memory but not the container's actual image identity.

## Decision

Ordinary runs, desktop readiness and noVNC extraction use `lab_eden::IMAGE`, the canonical
`carafe-lab-eden` image. The legacy `edenImage` JSON key remains readable for compatibility but has no
effect on execution and is removed by configuration writers. There is no replacement image profile.

After acquiring the emulator queue, a run resolves its selected image through local Docker inspection.
The immutable ID supplies launch and helper containers, retained-image tagging and the short ID in the
report. A rebuilt tag is picked up by the next run; one run does not mix emulator builds. Reuse additionally
requires the existing container's immutable image ID to match. Unknown identity prevents reuse.

Explicit historical `--like` replay keeps its retained runtime and image. Missing current or historical
images fail explicitly; they do not cause a fallback or automatic download. Control commands attaching to
an existing run do not replace its image. Opening the window still checks readiness without building or
starting Eden.

## Rejected alternatives

- Replacing one local override with another leaves the hidden selection mechanism in place.
- Comparing image tag strings misses a rebuild under the same tag.
- Resolving the tag separately for launch and retention can misidentify the build used by a run.
- Replaying old reports with the current image would silently invalidate historical comparisons.

## Verification

Configuration regression tests distinguish legacy overrides from ordinary settings and exercise all four
configuration writers. Image tests cover immutable ID parsing, duplicate or malformed inspection output,
identity-based reuse, pinned container arguments, canonical selection and explicit historical replay.
Real-game verification must compare the running container and report identity and inspect the picture;
correct image selection alone does not establish that the game renders or saves correctly.
