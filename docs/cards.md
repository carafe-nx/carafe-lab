# Game cards

What "the game works" means for each game the lab checks before a Carafe release. A card is the goal of a live
pass: the player — a person, or Claude through `play`, `pause`, `press` and `screen` — reaches every point in order,
and the game fails at the first fail condition. A pass that reached the end becomes the game's scenario, with a
reference picture at each point, so later checks replay it without a player.

Every card also fails on what the lab sees by itself: the game crashes or hangs (`[STALL]`), the emulator crashes
while the game runs, the screen stays black for a minute after the first frame, or a close loses save data.

## OpenTTD

- Points: main menu → new game on the default map → build a road and a bus depot → save → quit to the menu → load
  the save, the road is there.
- Fails: no main menu in 2 min, the map does not draw, the mouse pointer does not follow the stick, below 20 FPS on
  the map.

## Need for Speed Underground

- Points: intro or title → main menu → Quick Race → Circuit → the race starts → finish one lap → back to the menu.
- Fails: no menu in 3 min, the race does not load in 2 min, below 25 FPS while racing, a black or missing road,
  no engine sound.

## Need for Speed Underground 2

- Points: main menu → Quick Race → Circuit → the race starts → drive 90 s → pause menu → quit to the menu.
- Fails: no menu in 3 min, the race does not load in 2 min, below 20 FPS while racing, black asphalt, no
  speedometer or minimap.

## Need for Speed Carbon

- Points: main menu → Quick Race → Circuit → the race starts → drive 90 s → quit to the menu → the career save is
  still listed.
- Fails: no menu in 3 min, the race does not load in 2 min, below 20 FPS while racing, a crash at the start of a
  race.

## Dead Space

- Points: main menu → New Game → the first scene plays → the player walks → save at the first save station → quit
  to the menu → Load, the same place.
- Fails: no menu in 3 min, the first scene does not load in 3 min, below 20 FPS while walking, the level does not
  load.

## Heroes of Might and Magic IV

- Points: intro → main menu → New Game → a single scenario → the adventure map → move a hero three times → end the
  turn → save → load, the hero stands where he was.
- Fails: no menu in 3 min, a crash in the intro video, the cursor does not move, the map is frozen while frames go
  on.

## Wolverine (after the x87 fix)

- Points: main menu → New Game → the first fight → fight for a minute.
- Fails: below 10 FPS in the menu, "Fatal error", a hang after the first cut-scene.
