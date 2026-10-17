//! The live page: a small local web server that shows a run while it goes.
//!
//! It listens on `127.0.0.1` only. The page polls `/state` once a second; the emulator's screen is the noVNC
//! page the container publishes on `127.0.0.1:6080`.

use std::io::{BufRead, BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::{Component, Path, PathBuf};
use std::sync::{Arc, Mutex, PoisonError};
use std::thread;
use std::time::{Duration, Instant};

use carafe_core::cancel::CancelToken;
use serde::Serialize;

use lab_core::lang::Lang;
use lab_core::scenario::Step;
use lab_eden::{PressLog, eden_keys_json};
use lab_run::report;
use lab_run::run::{Outcome, StepResult};

/// First port the live page tries; the next nine are tried when it is taken.
pub const FIRST_PORT: u16 = 7300;
/// Address of the emulator's screen in the browser: the lab's own noVNC page, the picture alone.
pub const SCREEN_URL: &str = "http://127.0.0.1:6080/carafe.html?v=1";

/// One step as the live page shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveStep {
    /// The step's title in the run's language.
    pub title: String,
    /// What it found.
    pub outcome: Outcome,
    /// Details.
    pub detail: String,
    /// Seconds since the game's scenario started.
    pub at: f64,
}

/// A finished game as the live page shows it.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveGame {
    /// Its name.
    pub name: String,
    /// Its outcome.
    pub outcome: Outcome,
}

/// What the live page shows.
#[derive(Debug, Clone, Default, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LiveState {
    /// `ru` or `en`.
    pub lang: String,
    /// When the run started.
    pub started: String,
    /// Games or probes in the run.
    pub total: usize,
    /// Number of the current game, from one; zero before the first.
    pub index: usize,
    /// Name of the current game.
    pub game: String,
    /// `build`, `run` or `done` for the current game.
    pub phase: String,
    /// Steps of the current game so far.
    pub steps: Vec<LiveStep>,
    /// Games already finished.
    pub finished: Vec<LiveGame>,
    /// The run is over.
    pub done: bool,
    /// The viewer asked to stop the run, and it has not stopped yet.
    pub stopping: bool,
    /// The emulator's screen can be shown.
    pub screen: bool,
    /// Grows with every launch that started, so the page connects to each new emulator once its screen is up;
    /// zero before the first launch.
    pub screen_session: u64,
    /// Names of the buttons the lab holds now, such as `A` or `HOME`.
    pub pressed: Vec<String>,
}

#[derive(Default)]
struct Shared {
    state: LiveState,
    logs: Option<PathBuf>,
    run_dir: Option<PathBuf>,
    presses: Option<PressLog>,
    closing: bool,
    last_poll: Option<Instant>,
}

/// The running live page.
#[derive(Clone)]
pub struct Live {
    shared: Arc<Mutex<Shared>>,
    stop: CancelToken,
    port: u16,
}

/// Why [`Live::wait_for_close`] returned.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Closed {
    /// The viewer pressed "Close the lab".
    ByButton,
    /// The page stopped asking for news: its tab was closed.
    Tab,
    /// The longest wait ran out.
    TimeOut,
}

impl Live {
    /// Starts the page on the first free port from [`FIRST_PORT`].
    ///
    /// # Errors
    ///
    /// A message if none of the ports is free.
    pub fn start(lang: Lang, started: &str, total: usize, screen: bool) -> Result<Self, String> {
        let (listener, port) = (FIRST_PORT..FIRST_PORT + 10)
            .find_map(|port| {
                TcpListener::bind(("127.0.0.1", port))
                    .ok()
                    .map(|listener| (listener, port))
            })
            .ok_or_else(|| format!("ports {FIRST_PORT}…{} are all taken", FIRST_PORT + 9))?;
        let state = LiveState {
            lang: if lang == Lang::Ru { "ru" } else { "en" }.to_owned(),
            started: started.to_owned(),
            total,
            screen,
            ..LiveState::default()
        };
        let shared = Arc::new(Mutex::new(Shared {
            state,
            ..Shared::default()
        }));
        let stop = CancelToken::new();
        let serving = Arc::clone(&shared);
        let stopping = stop.clone();
        thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let shared = Arc::clone(&serving);
                let stop = stopping.clone();
                thread::spawn(move || {
                    let _ = serve(stream, &shared, &stop);
                });
            }
        });
        Ok(Self { shared, stop, port })
    }

    /// Returns the token the "Stop the run" button cancels; the run checks it between steps.
    #[must_use]
    pub fn stop_token(&self) -> CancelToken {
        self.stop.clone()
    }

    /// Keeps the page up after the run until the viewer closes the lab, the tab stops asking for news for
    /// `idle`, or `longest` runs out.
    pub fn wait_for_close(&self, idle: Duration, longest: Duration) -> Closed {
        let started = Instant::now();
        loop {
            {
                let shared = self.lock();
                if shared.closing {
                    return Closed::ByButton;
                }
                let quiet = shared
                    .last_poll
                    .map_or(started.elapsed(), |at| at.elapsed());
                if quiet >= idle {
                    return Closed::Tab;
                }
            }
            if started.elapsed() >= longest {
                return Closed::TimeOut;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }

    /// Returns the page's address.
    #[must_use]
    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}/", self.port)
    }

    /// Sets the folder of the run, from which the report is served.
    pub fn set_run_dir(&self, dir: &Path) {
        self.lock().run_dir = Some(dir.to_path_buf());
    }

    /// Shows the buttons from `presses` on the page's Switch while they are held.
    pub fn watch_presses(&self, presses: PressLog) {
        self.lock().presses = Some(presses);
    }

    /// Moves to the game number `index` and its build.
    pub fn begin_game(&self, index: usize, name: &str) {
        let mut shared = self.lock();
        shared.state.index = index;
        shared.state.game = name.to_owned();
        shared.state.phase = "build".to_owned();
        shared.state.steps.clear();
        shared.logs = None;
    }

    /// Moves the current game to its scenario; `logs` is the folder its runtime writes logs to.
    pub fn begin_run(&self, logs: &Path) {
        let mut shared = self.lock();
        shared.state.phase = "run".to_owned();
        shared.logs = Some(logs.to_path_buf());
    }

    /// Adds a step of the current game.
    pub fn step(&self, step: &StepResult, lang: Lang) {
        let live = LiveStep {
            title: report::step_title(step, lang),
            outcome: step.outcome,
            detail: step.detail.clone(),
            at: step.at,
        };
        let mut shared = self.lock();
        if step.step == Step::Launch && step.outcome == Outcome::Pass {
            shared.state.screen_session += 1;
        }
        shared.state.steps.push(live);
    }

    /// Records the end of the current game.
    pub fn end_game(&self, outcome: Outcome) {
        let mut shared = self.lock();
        let name = shared.state.game.clone();
        shared.state.phase = "done".to_owned();
        shared.state.finished.push(LiveGame { name, outcome });
    }

    /// Marks the run as over.
    pub fn finish(&self) {
        let mut shared = self.lock();
        shared.state.done = true;
        shared.state.stopping = false;
    }

    fn lock(&self) -> std::sync::MutexGuard<'_, Shared> {
        self.shared.lock().unwrap_or_else(PoisonError::into_inner)
    }
}

fn serve(stream: TcpStream, shared: &Mutex<Shared>, stop: &CancelToken) -> std::io::Result<()> {
    let mut reader = BufReader::new(stream.try_clone()?);
    let mut request = String::new();
    reader.read_line(&mut request)?;
    let mut words = request.split_whitespace();
    let method = words.next().unwrap_or("GET").to_owned();
    let path = words.next().unwrap_or("/").to_owned();
    let route = path.split('?').next().unwrap_or("/").to_owned();
    let mut stream = stream;
    if method == "POST" {
        let mut shared = shared.lock().unwrap_or_else(PoisonError::into_inner);
        match route.as_str() {
            "/stop" => {
                stop.cancel();
                shared.state.stopping = true;
            }
            "/close" => shared.closing = true,
            _ => {
                drop(shared);
                return respond(&mut stream, "404 Not Found", "text/plain", b"not found");
            }
        }
        drop(shared);
        return respond(&mut stream, "200 OK", "text/plain", b"ok");
    }
    let (mut state, logs, run_dir, presses) = {
        let mut shared = shared.lock().unwrap_or_else(PoisonError::into_inner);
        if route == "/state" {
            shared.last_poll = Some(Instant::now());
        }
        (
            shared.state.clone(),
            shared.logs.clone(),
            shared.run_dir.clone(),
            shared.presses.clone(),
        )
    };
    if let Some(presses) = presses {
        state.pressed = presses.held();
    }
    match route.as_str() {
        "/" => respond(
            &mut stream,
            "200 OK",
            "text/html; charset=utf-8",
            page().as_bytes(),
        ),
        "/state" => {
            let json = serde_json::to_vec(&state).unwrap_or_default();
            respond(&mut stream, "200 OK", "application/json", &json)
        }
        "/log" => {
            let text = logs.map(|dir| log_tail(&dir)).unwrap_or_default();
            respond(
                &mut stream,
                "200 OK",
                "text/plain; charset=utf-8",
                text.as_bytes(),
            )
        }
        file if file.starts_with("/run/") => match run_dir.and_then(|dir| inside(&dir, &file[5..]))
        {
            Some(path) => match std::fs::read(&path) {
                Ok(bytes) => respond(&mut stream, "200 OK", content_type(&path), &bytes),
                Err(_) => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
            },
            None => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
        },
        _ => respond(&mut stream, "404 Not Found", "text/plain", b"not found"),
    }
}

fn respond(stream: &mut TcpStream, status: &str, kind: &str, body: &[u8]) -> std::io::Result<()> {
    write!(
        stream,
        "HTTP/1.1 {status}\r\nContent-Type: {kind}\r\nContent-Length: {}\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n",
        body.len()
    )?;
    stream.write_all(body)
}

/// Returns the file `relative` inside `root`, or `None` if the path leaves `root` or names a folder above it.
#[must_use]
pub fn inside(root: &Path, relative: &str) -> Option<PathBuf> {
    let decoded = relative.replace("%20", " ");
    let candidate = Path::new(&decoded);
    let safe = candidate
        .components()
        .all(|component| matches!(component, Component::Normal(_)));
    safe.then(|| root.join(candidate))
}

fn content_type(path: &Path) -> &'static str {
    match path.extension().and_then(|ext| ext.to_str()) {
        Some("html") => "text/html; charset=utf-8",
        Some("png") => "image/png",
        Some("json") => "application/json",
        _ => "text/plain; charset=utf-8",
    }
}

fn log_tail(dir: &Path) -> String {
    let overlay = std::fs::read_to_string(dir.join("carafe-overlay.log")).unwrap_or_default();
    let runtime =
        std::fs::read_to_string(dir.join("logs").join("autorun_runtime.log")).unwrap_or_default();
    let lines = report::log_excerpt(&overlay, &runtime);
    let start = lines.len().saturating_sub(40);
    lines[start..].join("\n")
}

fn page() -> String {
    PAGE.replace("__SCREEN__", SCREEN_URL)
        .replace("__KEYS__", &eden_keys_json())
}

const PAGE: &str = r#"<!doctype html>
<html lang="ru">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>Carafe Lab</title>
<style>
:root{--font:"Inter","Segoe UI",system-ui,sans-serif;--mono:"Cascadia Mono",Consolas,monospace;--bg:#f6f1ef;--panel:#fff;
--text:#251b20;--muted:#7c6d73;--border:#e8dee1;--track:#ede3e6;--accent:#8e2846;--ok:#2f8f5b;--warn:#8a6100;--danger:#b3261e;color-scheme:light}
@media (prefers-color-scheme:dark){:root{--bg:#17121a;--panel:#251e27;--text:#f1e9ec;--muted:#a99ba2;--border:#352b35;
--track:#3a2f39;--accent:#db6e87;--ok:#5bc48a;--warn:#e2bc55;--danger:#f28b82;color-scheme:dark}}
*{box-sizing:border-box}
html,body{height:100%}
[hidden]{display:none!important}
body{margin:0;background:var(--bg);color:var(--text);font:14px/1.5 var(--font);padding:16px;display:grid;grid-template-columns:minmax(0,1fr) 380px;gap:16px}
@media (max-width:900px){body{grid-template-columns:1fr;height:auto}}
.stage{display:flex;flex-direction:column;align-items:center;justify-content:center;gap:12px;min-height:0}
.device{position:relative;width:min(100%,1100px);aspect-ratio:251/124}
.device svg{position:absolute;inset:0;width:100%;height:100%;overflow:visible}
.device .shell{fill:none;stroke:var(--text);stroke-width:.9}
.device .fixed{fill:none;stroke:var(--muted);stroke-width:.6;transition:fill 120ms,stroke 120ms}
.device .key{fill:var(--panel);stroke:var(--text);stroke-width:.7;transition:fill 120ms,stroke 120ms}
.device .ring{fill:none;stroke:var(--text);stroke-width:.7;transition:stroke 120ms}
.device .keyText{fill:var(--text);font:700 3.6px var(--font);text-anchor:middle;transition:fill 120ms}
.device .on .key,.device .on .fixed{fill:var(--accent);stroke:var(--accent)}
.device .on .ring{stroke:var(--accent)}
.device .on .keyText{fill:var(--panel)}
.screen{position:absolute;left:calc(49.5 / 251 * 100%);top:calc(24 / 124 * 100%);width:calc(152 / 251 * 100%);aspect-ratio:16/9;background:#000;border-radius:2px;overflow:hidden}
.screen iframe{border:0;width:100%;height:100%;display:block}
.screen .none{position:absolute;inset:0;display:grid;place-items:center;color:#a99ba2;padding:24px;text-align:center}
.stage label{font-size:12px;color:var(--muted)}
.side{display:flex;flex-direction:column;gap:12px;min-height:0}
.card{background:var(--panel);border:1px solid var(--border);border-radius:14px;padding:14px;display:flex;flex-direction:column;gap:8px;min-height:0}
.eyebrow{font-size:11px;text-transform:uppercase;letter-spacing:.08em;color:var(--accent);font-weight:600}
h1{font-size:18px;margin:0}
.muted{color:var(--muted)}
.progress{height:6px;background:var(--track);border-radius:3px;overflow:hidden}
.progress i{display:block;height:100%;background:var(--accent);width:0;transition:width .4s}
ol{list-style:none;margin:0;padding:0;overflow:auto;max-height:38vh}
li{display:grid;grid-template-columns:18px 1fr auto;gap:8px;padding:6px 0;border-bottom:1px solid var(--border)}
li:last-child{border-bottom:0}
.dot{width:16px;height:16px;border-radius:50%;display:grid;place-items:center;font-size:10px;font-weight:700;color:var(--panel);margin-top:2px}
.pass{background:var(--ok)}.warn{background:var(--warn)}.fail{background:var(--danger)}.skip,.console{background:var(--track);color:var(--muted)}
.what{font-weight:600}.how{font-size:12px;color:var(--muted);overflow-wrap:anywhere}
.time{font:12px var(--mono);color:var(--muted)}
.chips{display:flex;flex-wrap:wrap;gap:6px}
.chip{font-size:12px;padding:2px 8px;border-radius:999px;border:1px solid var(--border)}
.chip b{font-weight:600}
pre{margin:0;font:11.5px/1.5 var(--mono);overflow:auto;max-height:24vh;white-space:pre-wrap}
a{color:var(--accent)}
label{display:flex;gap:6px;align-items:center;cursor:pointer}
.buttons{display:flex;gap:8px;flex-wrap:wrap}
button{font:inherit;font-weight:600;padding:7px 14px;border-radius:10px;border:1px solid var(--border);background:var(--panel);color:var(--text);cursor:pointer}
button:hover{border-color:var(--accent)}
button:focus-visible{outline:2px solid var(--accent);outline-offset:2px}
button.danger{background:var(--danger);border-color:var(--danger);color:var(--panel)}
button:disabled{opacity:.5;cursor:default}
.gone .side .card:not(.controls){opacity:.6}
</style>
</head>
<body>
<section class="stage">
  <div class="device">
    <svg id="device" viewBox="-6 -16 251 124" aria-hidden="true"></svg>
    <div class="screen">
      <iframe id="vnc" title="Eden" hidden></iframe>
      <div class="none" id="no-screen"></div>
    </div>
  </div>
  <label><input type="checkbox" id="control"><span id="control-label"></span></label>
</section>
<aside class="side">
  <div class="card">
    <span class="eyebrow" id="eyebrow"></span>
    <h1 id="game"></h1>
    <span class="muted" id="phase"></span>
    <div class="progress"><i id="bar"></i></div>
  </div>
  <div class="card"><ol id="steps"></ol></div>
  <div class="card"><div class="chips" id="finished"></div><a id="report" href="/run/report.html" target="_blank"></a></div>
  <div class="card controls">
    <span class="muted" id="notice"></span>
    <div class="buttons">
      <button type="button" id="stop"></button>
      <button type="button" id="stop-sure" class="danger" hidden></button>
      <button type="button" id="close" hidden></button>
    </div>
  </div>
  <div class="card"><span class="eyebrow" id="log-title"></span><pre id="log"></pre></div>
</aside>
<script>
var screenUrl = "__SCREEN__";
var words = {
  ru: {lab: "Carafe Lab", screen: "Экран эмулятора", control: "Управлять", none: "Экран появится, когда игра запустится", build: "Сборка NSP", run: "Сценарий", done: "Готово", over: "Прогон закончен", waiting: "Ожидание", report: "Открыть отчёт", log: "Лог игры", of: "из",
    stop: "Остановить прогон", stopSure: "Да, остановить", stopAsk: "Текущий шаг доделается, игра и эмулятор закроются, отчёт запишется.", stopping: "Останавливаю: дожидаюсь конца шага…", close: "Закрыть стенд", closeHint: "Стенд держит эту страницу, пока её не закрыть кнопкой или вкладкой.", gone: "Стенд остановлен. Страницу можно закрыть."},
  en: {lab: "Carafe Lab", screen: "Emulator screen", control: "Control", none: "The screen shows once a game starts", build: "Building the NSP", run: "Scenario", done: "Done", over: "The run is over", waiting: "Waiting", report: "Open the report", log: "Game log", of: "of",
    stop: "Stop the run", stopSure: "Yes, stop", stopAsk: "The current step finishes, the game and the emulator close, the report is written.", stopping: "Stopping: waiting for the step to end…", close: "Close the lab", closeHint: "The lab keeps this page until you close it with the button or the tab.", gone: "The lab has stopped. You can close this page."}
};
var lang = "ru";
var gone = false;
var asking = false;
function post(path) { return fetch(path, {method: "POST"}).catch(function () {}); }
document.getElementById("stop").addEventListener("click", function () { asking = true; tick(); });
document.getElementById("stop-sure").addEventListener("click", function () { asking = false; post("/stop").then(tick); });
document.getElementById("close").addEventListener("click", function () { post("/close").then(function () { setTimeout(tick, 1500); }); });
function markGone() {
  if (gone) { return; }
  gone = true;
  var w = words[lang] || words.en;
  document.body.classList.add("gone");
  text("notice", w.gone);
  ["stop", "stop-sure", "close"].forEach(function (id) { document.getElementById(id).hidden = true; });
  setScreen(false);
}
function controls(s, w) {
  var stop = document.getElementById("stop");
  var sure = document.getElementById("stop-sure");
  var close = document.getElementById("close");
  stop.textContent = w.stop;
  sure.textContent = w.stopSure;
  close.textContent = w.close;
  stop.hidden = s.done || asking || s.stopping;
  sure.hidden = s.done || !asking || s.stopping;
  close.hidden = !s.done;
  text("notice", s.done ? w.closeHint : (s.stopping ? w.stopping : (asking ? w.stopAsk : "")));
}
var marks = {pass: "✓", warn: "!", fail: "×", skip: "–", console: "→"};
var vnc = document.getElementById("vnc");
var control = document.getElementById("control");
var shownScreen = null;
function text(id, value) { document.getElementById(id).textContent = value; }
function setScreen(on, session) {
  var url = screenUrl + (control.checked ? "" : "&view_only=1") + "&session=" + session;
  if (on && shownScreen !== url) { vnc.src = url; shownScreen = url; }
  vnc.hidden = !on;
  document.getElementById("no-screen").hidden = on;
}
control.addEventListener("change", function () { shownScreen = null; tick(); });
var keys = __KEYS__;
var labHeld = [];
var viewerHeld = {};
var svgNs = "http://www.w3.org/2000/svg";
var device = document.getElementById("device");
function draw(parent, tag, attrs, label) {
  var node = document.createElementNS(svgNs, tag);
  Object.keys(attrs).forEach(function (name) { node.setAttribute(name, attrs[name]); });
  if (label) { node.textContent = label; }
  parent.appendChild(node);
  return node;
}
function part(button) { return draw(device, "g", {"data-button": button}); }
function arrow(x, y, dx, dy, size) {
  var px = -dy, py = dx;
  return (x + dx * size) + "," + (y + dy * size) + " " +
    (x - dx * size * 0.65 + px * size) + "," + (y - dy * size * 0.65 + py * size) + " " +
    (x - dx * size * 0.65 - px * size) + "," + (y - dy * size * 0.65 - py * size);
}
var directions = {n: [0, -1], s: [0, 1], w: [-1, 0], e: [1, 0]};
function stick(cx, cy, ids) {
  var center = part(ids.c);
  draw(center, "circle", {"class": "ring", cx: cx, cy: cy, r: 10});
  draw(center, "circle", {"class": "key", cx: cx, cy: cy, r: 6.6});
  Object.keys(directions).forEach(function (dir) {
    var d = directions[dir];
    draw(part(ids[dir]), "polygon", {"class": "key", points: arrow(cx + d[0] * 13, cy + d[1] * 13, d[0], d[1], 1.5)});
  });
}
function four(cx, cy, ids, letters) {
  Object.keys(directions).forEach(function (dir) {
    var d = directions[dir];
    var x = cx + d[0] * 9.2, y = cy + d[1] * 9.2;
    var g = part(ids[dir]);
    draw(g, "circle", {"class": "key", cx: x, cy: y, r: 4.2});
    if (letters) { draw(g, "text", {"class": "keyText", x: x, y: y + 1.3}, ids[dir]); }
    else { draw(g, "polygon", {"class": "keyText", points: arrow(x, y, d[0], d[1], 1.4)}); }
  });
}
function drawSwitch() {
  var zl = part("ZL");
  draw(zl, "rect", {"class": "key", x: 9, y: -11.5, width: 24.5, height: 7.5, rx: 3.75});
  draw(zl, "text", {"class": "keyText", x: 21.25, y: -6.5}, "ZL");
  var zr = part("ZR");
  draw(zr, "rect", {"class": "key", x: 205.5, y: -11.5, width: 24.5, height: 7.5, rx: 3.75});
  draw(zr, "text", {"class": "keyText", x: 217.75, y: -6.5}, "ZR");
  draw(part("L"), "path", {"class": "key", d: "M4.6 5.8 A16.5 16.5 0 0 1 16.4 -2 H33.6 V-0.6 H16.6 A15 15 0 0 0 6 4.6 Z"});
  draw(part("R"), "path", {"class": "key", d: "M234.4 5.8 A16.5 16.5 0 0 0 222.6 -2 H205.4 V-0.6 H222.4 A15 15 0 0 1 233 4.6 Z"});
  draw(device, "path", {"class": "shell", d: "M35.9 0 H16 A16 16 0 0 0 0 16 V86 A16 16 0 0 0 16 102 H35.9 Z"});
  draw(device, "path", {"class": "shell", d: "M203.1 0 H223 A16 16 0 0 1 239 16 V86 A16 16 0 0 1 223 102 H203.1 Z"});
  draw(device, "rect", {"class": "shell", x: 35.9, y: 0, width: 167.2, height: 102});
  draw(part("MINUS"), "rect", {"class": "key", x: 25.8, y: 7.6, width: 6.4, height: 1.9, rx: 0.95});
  draw(part("PLUS"), "path", {"class": "key", d: "M209.05 6.3 a0.95 0.95 0 0 1 1.9 0 V7.6 H212.25 a0.95 0.95 0 0 1 0 1.9 H210.95 V10.8 a0.95 0.95 0 0 1 -1.9 0 V9.5 H207.75 a0.95 0.95 0 0 1 0 -1.9 H209.05 Z"});
  stick(17.5, 28, {n: "LUP", s: "LDOWN", w: "LLEFT", e: "LRIGHT", c: "STICKL"});
  stick(221.5, 60, {n: "RUP", s: "RDOWN", w: "RLEFT", e: "RRIGHT", c: "STICKR"});
  four(17.5, 60, {n: "UP", s: "DOWN", w: "LEFT", e: "RIGHT"}, false);
  four(221.5, 28, {n: "X", s: "B", w: "Y", e: "A"}, true);
  draw(part("CAPTURE"), "rect", {"class": "fixed", x: 22.6, y: 78.4, width: 5.6, height: 5.6, rx: 1.2});
  draw(part("HOME"), "circle", {"class": "fixed", cx: 213.6, cy: 81.2, r: 3.3});
}
function light() {
  var lit = {};
  labHeld.forEach(function (name) { lit[name] = true; });
  Object.keys(viewerHeld).forEach(function (name) { lit[name] = true; });
  device.querySelectorAll("[data-button]").forEach(function (g) {
    g.classList.toggle("on", !!lit[g.getAttribute("data-button")]);
  });
}
window.addEventListener("message", function (event) {
  var data = event.data || {};
  if (!data.carafeKey) { return; }
  var key = data.key.length === 1 ? data.key.toLowerCase() : data.key.replace(/^Arrow/, "");
  var button = keys[key];
  if (!button) { return; }
  if (data.carafeKey === "keydown") { viewerHeld[button] = true; } else { delete viewerHeld[button]; }
  light();
});
drawSwitch();
function tick() {
  if (gone) { return; }
  fetch("/state").then(function (r) { return r.json(); }).then(function (s) {
    lang = s.lang;
    var w = words[s.lang] || words.en;
    controls(s, w);
    document.documentElement.lang = s.lang;
    text("eyebrow", w.lab + " · " + s.started);
    text("control-label", w.control);
    labHeld = s.pressed || [];
    light();
    text("no-screen", w.none);
    text("game", s.index ? s.index + " " + w.of + " " + s.total + ": " + s.game : w.waiting);
    text("phase", s.done ? w.over : (w[s.phase] || ""));
    text("report", w.report);
    text("log-title", w.log);
    document.getElementById("bar").style.width = (s.total ? Math.round(100 * s.finished.length / s.total) : 0) + "%";
    setScreen(s.screen && s.phase === "run" && !s.done && s.screenSession > 0, s.screenSession);
    var steps = document.getElementById("steps");
    steps.replaceChildren.apply(steps, s.steps.map(function (step) {
      var li = document.createElement("li");
      var dot = document.createElement("span");
      dot.className = "dot " + step.outcome;
      dot.textContent = marks[step.outcome] || "";
      var body = document.createElement("span");
      var what = document.createElement("span");
      what.className = "what";
      what.textContent = step.title;
      var how = document.createElement("span");
      how.className = "how";
      how.textContent = step.detail;
      body.append(what, document.createElement("br"), how);
      var time = document.createElement("span");
      time.className = "time";
      var seconds = Math.round(step.at);
      time.textContent = Math.floor(seconds / 60) + ":" + String(seconds % 60).padStart(2, "0");
      li.append(dot, body, time);
      return li;
    }));
    steps.scrollTop = steps.scrollHeight;
    var finished = document.getElementById("finished");
    finished.replaceChildren.apply(finished, s.finished.map(function (game) {
      var chip = document.createElement("span");
      chip.className = "chip";
      var mark = document.createElement("b");
      mark.textContent = (marks[game.outcome] || "") + " ";
      chip.append(mark, game.name);
      return chip;
    }));
  }).catch(markGone);
  fetch("/log").then(function (r) { return r.text(); }).then(function (t) {
    var log = document.getElementById("log");
    log.textContent = t;
    log.scrollTop = log.scrollHeight;
  }).catch(function () {});
}
tick();
setInterval(tick, 1000);
</script>
</body>
</html>
"#;

#[cfg(test)]
mod tests {
    use std::io::Read;

    use super::*;
    use lab_core::scenario::Step;

    fn get(url: &str, path: &str) -> String {
        let address = url.trim_start_matches("http://").trim_end_matches('/');
        let mut stream = TcpStream::connect(address).unwrap();
        write!(stream, "GET {path} HTTP/1.1\r\nHost: x\r\n\r\n").unwrap();
        let mut text = String::new();
        stream.read_to_string(&mut text).unwrap();
        text
    }

    #[test]
    fn paths_cannot_leave_the_run_folder() {
        let root = Path::new("D:\\Bench\\runs\\1");
        assert_eq!(
            inside(root, "OpenTTD/menu.png"),
            Some(root.join("OpenTTD/menu.png"))
        );
        assert_eq!(inside(root, "../2/report.html"), None);
        assert_eq!(inside(root, "/etc/passwd"), None);
        assert_eq!(inside(root, "C:/Windows/win.ini"), None);
    }

    #[test]
    fn the_page_and_state_are_served() {
        let live = Live::start(Lang::En, "2026-10-12 10:00", 2, false).unwrap();
        live.begin_game(1, "OpenTTD");
        live.step(
            &StepResult {
                step: Step::Launch,
                outcome: Outcome::Pass,
                detail: "launch 1 started".to_owned(),
                at: 1.0,
                screenshot: None,
                presses: Vec::new(),
            },
            Lang::En,
        );
        let page = get(&live.url(), "/");
        assert!(page.starts_with("HTTP/1.1 200 OK"));
        assert!(page.contains("127.0.0.1:6080/carafe.html"));
        assert!(page.contains("\"c\":\"A\""));
        assert!(!page.contains("__KEYS__"));
        let state = get(&live.url(), "/state");
        assert!(state.contains("\"game\":\"OpenTTD\""), "{state}");
        assert!(state.contains("\"title\":\"Launch\""));
        assert!(get(&live.url(), "/run/../secret").starts_with("HTTP/1.1 404"));
    }
}
