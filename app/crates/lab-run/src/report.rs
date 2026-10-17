//! The report of a run: `report.json` for tools and `report.html` for people.

use std::fmt::Write as _;

use lab_core::lang::Lang;
use serde::{Deserialize, Serialize};
use ts_rs::TS;

/// What a run was made with, so two runs can be compared and a run repeated with the same parts.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize, TS)]
#[serde(rename_all = "camelCase")]
#[ts(export)]
pub struct RunStamp {
    /// The Carafe checkout's commit; `+changes` follows it when the checkout has uncommitted changes.
    pub carafe_commit: Option<String>,
    /// The first 12 hex digits of the SHA-256 of the runtime archive the NSPs were packed with.
    pub runtime_hash: Option<String>,
    /// The lab's own commit, which holds Eden's patches.
    pub lab_commit: Option<String>,
    /// The short ID of the emulator's Docker image.
    pub eden_image: Option<String>,
}

use crate::run::{Outcome, ScenarioResult, StepResult, stage_name};
use lab_core::input_check;
use lab_core::milestones::Stage;
use lab_core::scenario::{Milestone, Step};

/// How the NSP of a game was built.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase", tag = "kind")]
pub enum BuildOutcome {
    /// The NSP is built.
    #[serde(rename_all = "camelCase")]
    Built {
        /// Seconds the build took.
        seconds: f64,
        /// Size of the NSP.
        size_bytes: u64,
    },
    /// The build failed.
    Failed {
        /// Why.
        error: String,
    },
}

/// Everything the run found for one game.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GameReport {
    /// Folder name of the game.
    pub name: String,
    /// Title shown on the Switch.
    pub title: String,
    /// Title ID.
    pub title_id: String,
    /// Folder of the game inside the run folder.
    pub dir: String,
    /// The settings in short, such as `dxvk official fex horizon …`.
    pub settings: String,
    /// The build.
    pub build: BuildOutcome,
    /// The scenario; `None` if the build failed.
    pub scenario: Option<ScenarioResult>,
    /// Milestone lines of every launch, for the report.
    pub log_excerpt: Vec<String>,
}

impl GameReport {
    /// Returns the outcome of the game: a failed build fails it, otherwise the worst step decides.
    #[must_use]
    pub fn outcome(&self) -> Outcome {
        match (&self.build, &self.scenario) {
            (BuildOutcome::Failed { .. }, _) => Outcome::Fail,
            (_, Some(scenario)) => scenario.outcome(),
            (_, None) => Outcome::Skip,
        }
    }

    fn furthest_stage(&self) -> Option<Stage> {
        self.scenario
            .as_ref()?
            .launches
            .iter()
            .map(|record| record.launch.stage())
            .max()
    }
}

/// A run: when, with what, and every game.
#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct RunReport {
    /// When the run started, `YYYY-MM-DD HH:MM` in UTC.
    pub started: String,
    /// Runtime version the NSPs were built with.
    pub runtime_version: String,
    /// The emulator and its version.
    pub emulator: String,
    /// What the run was made with.
    pub stamp: RunStamp,
    /// Seconds the whole run took.
    pub seconds: f64,
    /// The games in run order.
    pub games: Vec<GameReport>,
}

impl RunReport {
    /// Returns how many games ended with each outcome: pass, warning, console, fail.
    #[must_use]
    pub fn counts(&self) -> [usize; 4] {
        let mut counts = [0; 4];
        for game in &self.games {
            let slot = match game.outcome() {
                Outcome::Pass | Outcome::Skip => 0,
                Outcome::Warn => 1,
                Outcome::Console => 2,
                Outcome::Fail => 3,
            };
            counts[slot] += 1;
        }
        counts
    }

    /// Renders `report.html`: one self-contained page; screenshots are linked by relative paths.
    #[must_use]
    pub fn html(&self, lang: Lang) -> String {
        let mut out = String::with_capacity(32 * 1024);
        let title = lang.pick("Прогон Carafe", "Carafe run");
        let _ = write!(
            out,
            "<!doctype html>\n<html lang=\"{}\">\n<head>\n<meta charset=\"utf-8\">\n\
             <meta name=\"viewport\" content=\"width=device-width, initial-scale=1\">\n\
             <title>{title} {}</title>\n<style>{STYLE}</style>\n</head>\n<body>\n<div class=\"wrap\">\n",
            lang.pick("ru", "en"),
            escape(&self.started),
        );
        let [pass, warn, console, fail] = self.counts();
        let _ = writeln!(
            out,
            "<header><span class=\"eyebrow\">Carafe · Lab</span><h1>{title} {started}</h1>\
             <p class=\"muted\">{runtime} {version} · {emulator} · {minutes}</p>\
             <div class=\"chips\">{p}{w}{c}{f}</div></header>",
            started = escape(&self.started),
            runtime = lang.pick("Рантайм", "Runtime"),
            version = escape(&self.runtime_version),
            emulator = escape(&self.emulator),
            minutes = duration(self.seconds, lang),
            p = chip("ok", &format!("{} {pass}", lang.pick("прошли", "passed"))),
            w = chip(
                "warn",
                &format!("{} {warn}", lang.pick("с замечаниями", "with warnings"))
            ),
            c = chip(
                "na",
                &format!("{} {console}", lang.pick("на консоль", "for the console"))
            ),
            f = chip("bad", &format!("{} {fail}", lang.pick("упали", "failed"))),
        );
        out.push_str("<div class=\"panel app\">\n<nav class=\"side\" role=\"tablist\">\n");
        for (index, game) in self.games.iter().enumerate() {
            let (class, label) = outcome_chip(game.outcome(), lang);
            let summary = game_summary(game, lang);
            let _ = writeln!(
                out,
                "<button class=\"game\" role=\"tab\" data-game=\"{index}\" aria-selected=\"{selected}\">\
                 <span><span class=\"name\">{name}</span><br><span class=\"sub\">{summary}</span></span>{chip}</button>",
                selected = index == 0,
                name = escape(&game.title),
                summary = escape(&summary),
                chip = chip(class, label),
            );
        }
        out.push_str("</nav>\n");
        for (index, game) in self.games.iter().enumerate() {
            let hidden = if index == 0 { "" } else { " hidden" };
            let _ = writeln!(
                out,
                "<section class=\"detail\" data-game=\"{index}\"{hidden}>"
            );
            game_detail(&mut out, game, lang);
            out.push_str("</section>\n");
        }
        out.push_str("</div>\n</div>\n<script>");
        out.push_str(SCRIPT);
        out.push_str("</script>\n</body>\n</html>\n");
        out
    }
}

fn game_summary(game: &GameReport, lang: Lang) -> String {
    match &game.build {
        BuildOutcome::Failed { .. } => lang.pick("не собралась", "build failed").to_owned(),
        BuildOutcome::Built { .. } => {
            let Some(scenario) = &game.scenario else {
                return String::new();
            };
            let failed = scenario
                .steps
                .iter()
                .find(|step| matches!(step.outcome, Outcome::Fail | Outcome::Console));
            match failed {
                Some(step) => step.detail.clone(),
                None => {
                    let total = scenario.steps.last().map_or(0.0, |step| step.at);
                    format!(
                        "{} · {}",
                        lang.pick("все шаги", "all steps"),
                        duration(total, lang)
                    )
                }
            }
        }
    }
}

fn game_detail(out: &mut String, game: &GameReport, lang: Lang) {
    let (class, label) = outcome_chip(game.outcome(), lang);
    let _ = write!(
        out,
        "<div class=\"head\"><h2>{}</h2>{}</div>\n<p class=\"muted mono\">{} · {} · {}</p>\n",
        escape(&game.title),
        chip(class, label),
        escape(&game.name),
        escape(&game.title_id),
        escape(&game.settings),
    );
    out.push_str("<ol class=\"steps\">\n");
    match &game.build {
        BuildOutcome::Built {
            seconds,
            size_bytes,
        } => step_row(
            out,
            Outcome::Pass,
            lang.pick("Сборка NSP", "NSP build"),
            &format!("{} MB", size_bytes / 1_000_000),
            Some(*seconds),
        ),
        BuildOutcome::Failed { error } => step_row(
            out,
            Outcome::Fail,
            lang.pick("Сборка NSP", "NSP build"),
            error,
            None,
        ),
    }
    if let Some(scenario) = &game.scenario {
        for step in &scenario.steps {
            step_row(
                out,
                step.outcome,
                &step_title(step, lang),
                &step.detail,
                Some(step.at),
            );
        }
    }
    out.push_str("</ol>\n");
    if let Some(scenario) = &game.scenario {
        for step in scenario
            .steps
            .iter()
            .filter(|step| !step.presses.is_empty())
        {
            let _ = write!(
                out,
                "<h3>{}</h3>\n<ul class=\"presses\">\n",
                lang.pick("Кнопка → что получила игра", "Button → what the game got")
            );
            for press in &step.presses {
                let (class, mark) = if press.ok() {
                    ("ok", "✓")
                } else {
                    ("bad", "×")
                };
                let observed = if press.ok() {
                    String::new()
                } else {
                    format!(
                        " <span class=\"muted\">{} {}</span>",
                        lang.pick("ждали", "expected"),
                        escape(&input_check::describe(&press.expected))
                    )
                };
                let _ = writeln!(
                    out,
                    "<li><span class=\"dot {class}\">{mark}</span><b>{}</b> {}{observed}</li>",
                    escape(press.button.autorun_name()),
                    escape(&input_check::describe(&press.observed)),
                );
            }
            out.push_str("</ul>\n");
        }
    }
    if let Some(stage) = game.furthest_stage() {
        let _ = writeln!(
            out,
            "<p class=\"muted\">{}: {}</p>",
            lang.pick("Дальше всего дошла до", "Got as far as"),
            escape(stage_name(stage)),
        );
    }
    if let Some(scenario) = &game.scenario {
        let shots: Vec<&StepResult> = scenario
            .steps
            .iter()
            .filter(|step| {
                matches!(step.step, Step::Screenshot { .. }) && step.screenshot.is_some()
            })
            .collect();
        if !shots.is_empty() {
            out.push_str("<div class=\"shots\">\n");
            for step in shots {
                let (Step::Screenshot { name }, Some(file)) = (&step.step, &step.screenshot) else {
                    continue;
                };
                let src = format!("{}/{}", game.dir, file);
                let _ = writeln!(
                    out,
                    "<figure><a href=\"{src}\"><img src=\"{src}\" alt=\"{name}\" loading=\"lazy\"></a>\
                     <figcaption>{name}</figcaption></figure>",
                    src = escape(&src),
                    name = escape(name),
                );
            }
            out.push_str("</div>\n");
        }
        if !scenario.save.is_empty() {
            let _ = write!(
                out,
                "<h3>{}</h3>\n<ul class=\"files mono\">\n",
                lang.pick("Сохранения", "Save data")
            );
            for file in &scenario.save {
                let _ = writeln!(
                    out,
                    "<li>{} <span class=\"muted\">{} B · {}</span></li>",
                    escape(&file.path),
                    file.size,
                    escape(&file.sha256[..file.sha256.len().min(12)]),
                );
            }
            out.push_str("</ul>\n");
        }
        if !scenario.emulator_gaps.is_empty() {
            let _ = write!(
                out,
                "<h3>{}</h3>\n<pre class=\"log\">",
                lang.pick("Чего не хватает эмулятору", "What the emulator lacks")
            );
            for line in &scenario.emulator_gaps {
                let _ = writeln!(out, "{}", escape(line));
            }
            out.push_str("</pre>\n");
        }
    }
    if !game.log_excerpt.is_empty() {
        let _ = write!(
            out,
            "<h3>{}</h3>\n<pre class=\"log\">",
            lang.pick("Лог", "Log")
        );
        for line in &game.log_excerpt {
            let class = if line.contains("time limit")
                || line.contains("no progress")
                || line.contains("exit_code=0xc")
            {
                "er"
            } else if line.contains("hidden after") {
                "hl"
            } else {
                ""
            };
            if class.is_empty() {
                let _ = writeln!(out, "{}", escape(line));
            } else {
                let _ = writeln!(out, "<span class=\"{class}\">{}</span>", escape(line));
            }
        }
        out.push_str("</pre>\n");
    }
    let _ = writeln!(
        out,
        "<p class=\"muted\"><a href=\"{dir}/\">{}</a></p>",
        lang.pick(
            "Папка игры: логи всех запусков и скриншоты",
            "Game folder: logs of every launch and screenshots"
        ),
        dir = escape(&game.dir),
    );
}

fn step_row(out: &mut String, outcome: Outcome, title: &str, detail: &str, at: Option<f64>) {
    let (class, mark) = match outcome {
        Outcome::Pass => ("ok", "✓"),
        Outcome::Warn => ("warn", "!"),
        Outcome::Fail => ("bad", "×"),
        Outcome::Console => ("na", "→"),
        Outcome::Skip => ("na", "–"),
    };
    let time = at.map_or_else(|| "—".to_owned(), clock);
    let _ = writeln!(
        out,
        "<li class=\"step\"><span class=\"dot {class}\">{mark}</span><span><span class=\"what\">{}</span><br>\
         <span class=\"how\">{}</span></span><span class=\"time\">{time}</span></li>",
        escape(title),
        escape(detail),
    );
}

/// Returns the title of a step in the report's language.
#[must_use]
pub fn step_title(step: &StepResult, lang: Lang) -> String {
    match &step.step {
        Step::Launch => lang.pick("Запуск", "Launch").to_owned(),
        Step::WaitFor { milestone, .. } => {
            let what = match milestone {
                Milestone::Wine => lang.pick("Wine запустился", "Wine started"),
                Milestone::LoadingScreen => lang.pick("Загрузочный экран", "Loading screen"),
                Milestone::FirstFrame => lang.pick("Первый кадр игры", "The game's first frame"),
                Milestone::Commit => lang.pick("Запись сохранения", "Save data commit"),
            };
            what.to_owned()
        }
        Step::Wait { seconds } => format!(
            "{} {seconds} {}",
            lang.pick("Пауза", "Wait"),
            lang.pick("с", "s")
        ),
        Step::Press { buttons, .. } => {
            let names: Vec<&str> = buttons.iter().map(|button| button.autorun_name()).collect();
            format!("{} {}", lang.pick("Нажатие", "Press"), names.join("+"))
        }
        Step::Screenshot { name } => format!("{} {name}", lang.pick("Скриншот", "Screenshot")),
        Step::ExpectChange { from, to, .. } => format!(
            "{} {from} → {to}",
            lang.pick("Ввод дошёл до игры", "Input reached the game")
        ),
        Step::ExpectPattern { name } => format!(
            "{} {name}",
            lang.pick("Эталонная картинка на", "Reference picture in")
        ),
        Step::ExpectInput => lang
            .pick(
                "Кнопки дошли до игры по раскладке",
                "Buttons reached the game as bound",
            )
            .to_owned(),
        Step::ExpectSave => lang
            .pick("Игра записала сохранение", "The game wrote save data")
            .to_owned(),
        Step::ExpectSaveKept => lang
            .pick(
                "Сохранение пережило перезапуск",
                "Save data survived the restart",
            )
            .to_owned(),
        Step::Close { .. } => lang
            .pick("Закрытие, как из меню HOME", "Close, as from the HOME Menu")
            .to_owned(),
    }
}

fn outcome_chip(outcome: Outcome, lang: Lang) -> (&'static str, &'static str) {
    match outcome {
        Outcome::Pass => ("ok", if lang == Lang::Ru { "ОК" } else { "OK" }),
        Outcome::Skip => (
            "na",
            if lang == Lang::Ru {
                "Пропущена"
            } else {
                "Skipped"
            },
        ),
        Outcome::Warn => (
            "warn",
            if lang == Lang::Ru {
                "Замечания"
            } else {
                "Warnings"
            },
        ),
        Outcome::Console => (
            "na",
            if lang == Lang::Ru {
                "На консоль"
            } else {
                "Console"
            },
        ),
        Outcome::Fail => (
            "bad",
            if lang == Lang::Ru {
                "Упала"
            } else {
                "Failed"
            },
        ),
    }
}

fn chip(class: &str, label: &str) -> String {
    format!("<span class=\"pill {class}\">{}</span>", escape(label))
}

fn clock(seconds: f64) -> String {
    let whole = seconds.max(0.0).round() as u64;
    format!("{}:{:02}", whole / 60, whole % 60)
}

fn duration(seconds: f64, lang: Lang) -> String {
    let whole = seconds.max(0.0).round() as u64;
    let (minutes, rest) = (whole / 60, whole % 60);
    match lang {
        Lang::Ru if minutes > 0 => format!("{minutes} мин {rest} с"),
        Lang::Ru => format!("{rest} с"),
        Lang::En if minutes > 0 => format!("{minutes} min {rest} s"),
        Lang::En => format!("{rest} s"),
    }
}

/// Escapes text for HTML.
#[must_use]
pub fn escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&#39;"),
            other => out.push(other),
        }
    }
    out
}

/// Returns the milestone lines of a launch's logs for the report: the overlay's own lines and the runtime's
/// `[INIT]`, `[CARAFE]` and `[EXIT]` lines without the repeated session closing ones.
#[must_use]
pub fn log_excerpt(overlay: &str, runtime: &str) -> Vec<String> {
    let mut lines: Vec<String> = overlay
        .lines()
        .filter(|line| line.starts_with("[carafe-overlay]"))
        .map(str::to_owned)
        .collect();
    for line in runtime.lines() {
        let wanted = line.starts_with("[INIT] virtual memory ready")
            || line.starts_with("[INIT] Mesa")
            || line.starts_with("[EXIT] NtTerminateProcess")
            || line.starts_with("[EXIT] the program")
            || line.starts_with("[CARAFE] no progress");
        if wanted {
            lines.push(line.to_owned());
        }
    }
    lines
}

const STYLE: &str = r#"
:root{--font:"Inter","Segoe UI",system-ui,sans-serif;--mono:"Cascadia Mono",Consolas,ui-monospace,monospace;
--bg:#f6f1ef;--panel:#ffffffb8;--panel-solid:#fff;--text:#251b20;--muted:#7c6d73;--border:#e8dee1;
--border-strong:#d9ccd0;--track:#ede3e6;--accent:#8e2846;--accent-soft:rgba(142,40,70,.1);--ring:rgba(142,40,70,.28);
--ok:#2f8f5b;--ok-soft:rgba(47,143,91,.12);--warn:#8a6100;--warn-soft:rgba(184,138,30,.14);--danger:#b3261e;
--danger-soft:rgba(179,38,30,.1);color-scheme:light}
@media (prefers-color-scheme:dark){:root{--bg:#17121a;--panel:#28202ab8;--panel-solid:#251e27;--text:#f1e9ec;
--muted:#a99ba2;--border:#352b35;--border-strong:#4a3d49;--track:#3a2f39;--accent:#db6e87;--accent-soft:rgba(219,110,135,.14);
--ring:rgba(219,110,135,.32);--ok:#5bc48a;--ok-soft:rgba(91,196,138,.13);--warn:#e2bc55;--warn-soft:rgba(226,188,85,.14);
--danger:#f28b82;--danger-soft:rgba(242,139,130,.12);color-scheme:dark}}
*{box-sizing:border-box}
body{margin:0;background:var(--bg);color:var(--text);font:15px/1.55 var(--font);padding:28px 16px 64px}
.wrap{max-width:1180px;margin:0 auto;display:flex;flex-direction:column;gap:24px}
h1,h2,h3{margin:0;letter-spacing:-.01em;text-wrap:balance}h1{font-size:28px}h2{font-size:20px}h3{font-size:15px;margin-top:8px}
p{margin:0}.muted{color:var(--muted)}.mono{font-family:var(--mono);font-size:12.5px}
a{color:var(--accent)}
header{display:flex;flex-direction:column;gap:8px}
.eyebrow{font-size:12px;text-transform:uppercase;letter-spacing:.08em;color:var(--accent);font-weight:600}
.chips{display:flex;gap:8px;flex-wrap:wrap}
.pill{display:inline-flex;align-items:center;font-size:12px;font-weight:600;padding:3px 10px;border-radius:999px;white-space:nowrap}
.pill.ok{background:var(--ok-soft);color:var(--ok)}.pill.warn{background:var(--warn-soft);color:var(--warn)}
.pill.bad{background:var(--danger-soft);color:var(--danger)}.pill.na{background:var(--track);color:var(--muted)}
.panel{background:var(--panel);border:1px solid var(--border);border-radius:16px}
.app{display:grid;grid-template-columns:300px minmax(0,1fr);overflow:hidden}
@media (max-width:760px){.app{grid-template-columns:1fr}}
.side{border-right:1px solid var(--border);padding:12px;display:flex;flex-direction:column;gap:6px}
.game{all:unset;cursor:pointer;display:flex;justify-content:space-between;align-items:center;gap:8px;padding:10px 12px;border-radius:11px;border:1px solid transparent}
.game:hover{background:var(--accent-soft)}.game[aria-selected="true"]{background:var(--panel-solid);border-color:var(--border-strong)}
.game:focus-visible{outline:2px solid var(--ring)}.game .name{font-weight:600}.game .sub{font-size:12px;color:var(--muted)}
.detail{padding:18px;display:flex;flex-direction:column;gap:14px;min-width:0}
.head{display:flex;justify-content:space-between;align-items:center;gap:10px;flex-wrap:wrap}
.steps{list-style:none;margin:0;padding:0}
.step{display:grid;grid-template-columns:22px 1fr auto;gap:10px;align-items:start;padding:9px 4px;border-bottom:1px solid var(--border)}
.step:last-child{border-bottom:0}
.dot{width:18px;height:18px;border-radius:50%;display:grid;place-items:center;font-size:11px;font-weight:700;margin-top:2px;color:var(--panel-solid)}
.dot.ok{background:var(--ok)}.dot.warn{background:var(--warn)}.dot.bad{background:var(--danger)}.dot.na{background:var(--track);color:var(--muted)}
.what{font-weight:550}.how{font-size:13px;color:var(--muted);overflow-wrap:anywhere}
.time{font-family:var(--mono);font-size:12px;color:var(--muted);font-variant-numeric:tabular-nums}
.shots{display:grid;grid-template-columns:repeat(auto-fill,minmax(220px,1fr));gap:12px}
figure{margin:0;display:flex;flex-direction:column;gap:6px}
figure img{width:100%;aspect-ratio:16/9;object-fit:contain;background:#000;border-radius:10px;display:block}
figcaption{font-size:12px;color:var(--muted)}
.files{margin:0;padding-left:18px}
.presses{list-style:none;margin:0;padding:0;display:grid;grid-template-columns:repeat(auto-fill,minmax(260px,1fr));gap:6px 16px;font-size:13px}
.presses li{display:flex;gap:8px;align-items:center}
.presses .dot{margin-top:0;flex:none}
pre.log{margin:0;font:12px/1.6 var(--mono);background:var(--panel-solid);border:1px solid var(--border);border-radius:11px;padding:12px;overflow-x:auto}
.log .hl{color:var(--accent);font-weight:600}.log .er{color:var(--danger);font-weight:600}
"#;

const SCRIPT: &str = r#"
document.querySelectorAll('.game').forEach(function(button){
  button.addEventListener('click',function(){
    var id=button.dataset.game;
    document.querySelectorAll('.game').forEach(function(b){b.setAttribute('aria-selected',String(b===button));});
    document.querySelectorAll('.detail').forEach(function(d){d.hidden=d.dataset.game!==id;});
  });
});
"#;

#[cfg(test)]
mod tests {
    use super::*;
    use crate::run::LaunchRecord;
    use lab_core::milestones::Launch;

    fn passed(name: &str) -> GameReport {
        GameReport {
            name: name.to_owned(),
            title: format!("{name} <title>"),
            title_id: "0563ab630a362000".to_owned(),
            dir: name.to_owned(),
            settings: "dxvk official fex".to_owned(),
            build: BuildOutcome::Built {
                seconds: 12.0,
                size_bytes: 412_000_000,
            },
            scenario: Some(ScenarioResult {
                steps: vec![StepResult {
                    step: Step::Screenshot {
                        name: "menu".to_owned(),
                    },
                    outcome: Outcome::Pass,
                    detail: "saved menu.png".to_owned(),
                    at: 61.0,
                    screenshot: Some("menu.png".to_owned()),
                    presses: Vec::new(),
                }],
                launches: vec![LaunchRecord {
                    number: 1,
                    launch: Launch::default(),
                    logs: "launch-1".to_owned(),
                }],
                save: Vec::new(),
                emulator_gaps: Vec::new(),
            }),
            log_excerpt: vec!["[carafe-overlay] [CARAFE] loading screen hidden after 34 s: the game drew its picture".to_owned()],
        }
    }

    fn run(games: Vec<GameReport>) -> RunReport {
        RunReport {
            started: "2026-10-12 10:00".to_owned(),
            runtime_version: "0.3.0".to_owned(),
            emulator: "Eden v0.2.1".to_owned(),
            stamp: RunStamp::default(),
            seconds: 125.0,
            games,
        }
    }

    #[test]
    fn a_failed_build_fails_the_game() {
        let mut game = passed("A");
        game.build = BuildOutcome::Failed {
            error: "no keys".to_owned(),
        };
        assert_eq!(game.outcome(), Outcome::Fail);
        assert_eq!(run(vec![game, passed("B")]).counts(), [1, 0, 0, 1]);
    }

    #[test]
    fn the_page_escapes_titles_and_links_screenshots() {
        let html = run(vec![passed("OpenTTD")]).html(Lang::En);
        assert!(html.contains("OpenTTD &lt;title&gt;"));
        assert!(!html.contains("<title>OpenTTD"));
        assert!(html.contains("src=\"OpenTTD/menu.png\""));
        assert!(html.contains("class=\"hl\""));
        assert!(html.contains("1:01"));
    }

    #[test]
    fn the_page_speaks_both_languages() {
        let report = run(vec![passed("OpenTTD")]);
        assert!(report.html(Lang::Ru).contains("Скриншот menu"));
        assert!(report.html(Lang::En).contains("Screenshot menu"));
        assert!(report.html(Lang::Ru).contains("2 мин 5 с"));
    }

    #[test]
    fn only_the_first_game_is_shown_at_first() {
        let html = run(vec![passed("A"), passed("B")]).html(Lang::En);
        assert!(html.contains("<section class=\"detail\" data-game=\"0\">"));
        assert!(html.contains("<section class=\"detail\" data-game=\"1\" hidden>"));
    }

    #[test]
    fn the_excerpt_keeps_milestones_only() {
        let runtime = "[INIT] core balancing on\n[INIT] virtual memory ready\n[EXIT] closing the gpu session, 20 pages still with the GPU\n[EXIT] NtTerminateProcess(self) exit_code=0x00000000\n";
        let overlay = "[carafe-overlay] started 2026-10-10 04:32:36\nnoise\n";
        assert_eq!(
            log_excerpt(overlay, runtime),
            vec![
                "[carafe-overlay] started 2026-10-10 04:32:36".to_owned(),
                "[INIT] virtual memory ready".to_owned(),
                "[EXIT] NtTerminateProcess(self) exit_code=0x00000000".to_owned(),
            ]
        );
    }

    #[test]
    fn escape_covers_html_specials() {
        assert_eq!(
            escape("<a href=\"x\">'&'</a>"),
            "&lt;a href=&quot;x&quot;&gt;&#39;&amp;&#39;&lt;/a&gt;"
        );
    }
}
