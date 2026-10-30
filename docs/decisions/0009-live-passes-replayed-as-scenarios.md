# 9. A player passes each game once, paused; the lab replays the pass as a scenario

Date: 2026-10-31

## Context

A fixed scenario — launch, wait for the first frame, press A on a timer, close — proves little: it passes while the
game never reaches gameplay, and it breaks on any screen it did not expect. Before a Carafe release every game has to
show that it really works: it reaches gameplay, plays for minutes, saves and loads, and keeps a usable frame rate.

What general game-playing agents can do, from published results (October 2026):

- Played live, frontier models finish under 1 % of the games of VideoGameBench; paused while they think, they do
  better. Typical failures are the right button at the wrong moment, loops and imagined progress.
- Agents that do play real-time 3D games well need server GPUs (Lumine: four NVIDIA H20) or are closed (SIMA 2,
  Game-TARS). NVIDIA's open NitroGen only reacts to the last frame and cannot follow a goal.
- A vision model on this laptop's NPU (Qwen3-VL-8B) takes up to 6.6 s to its first token and shares the 22 W the CPU
  has with Eden.
- Models judge a screenshot far better than they play: they tell a glitch in about 83 % of pictures.
- Studio tools describe a test as a goal in plain words with explicit fail conditions, and judge by the picture and
  the logs.

## Decision

- Every game has a card in [cards.md](../cards.md): the points to reach in order and what fails it.
- The first pass of a card is played live through the lab's commands: `play`, `pause`, `press`, `screen`, `perf`,
  `logs`. The game stands paused while the player looks and decides (`patches/eden/0059`); `press` lets it go on only
  for the press and `--after` seconds. The player is a person or Claude.
- A pass that reached the end is written down as the game's scenario: its presses with their timings, an
  `expectScreen` with a reference picture at each point and `expectFps` where the frame rate matters. Later checks
  replay it without a player; a point whose picture does not match calls the player again.
- A check starts from a save near the point it checks, through the game's `save` folder in `lab.json`, instead of
  playing through intros and menus each time.
- Manual work stays as it was: plans, A/B runs, hand-written scenarios and the live commands.

## Rejected

- Training a model to play: it needs NVIDIA GPUs and hours of recorded play for every game.
- A vision model running on the laptop during a run: seconds per decision, and it takes power from Eden, so the
  frame rate in the report would be wrong.
- One long recording of a whole pass replayed blind: Wine and FEX timings drift, and the recording falls out of step
  with the game. Replays go in short pieces, each from a recognised screen.
- An agent playing real-time games live without pausing: the published results show it does not work yet.
