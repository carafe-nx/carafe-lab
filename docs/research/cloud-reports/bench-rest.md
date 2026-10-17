# bench-rest

- `bench/` — исходники стенда carafe-bench (без git): `src/`, `docker/`, `patches/eden/`, `tools/`, `docs/`,
  `ROADMAP.md`, `bench.example.json`. Положить рядом с репозиторием Carafe так, чтобы `../carafe` указывал на него.
- `runs/2026-10-14_07-51-41` — обрыв по таймауту, второй запуск без OpenGL, потерян `hwaccel.dat` (пункты 1, 2).
- `runs/2026-10-14_14-21-42`, `runs/2026-10-14_18-10-12` — закрытие не уложилось в 60 с, нет save data,
  реестр переписан после старта, ошибки RenameFile (пункты 2, 3, 4).
- В каждом прогоне: `report.json`, `report.html`, по запуску `eden_log.txt`, `carafe-overlay.log`, `logs/`.
