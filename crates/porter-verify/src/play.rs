//! Plays a ported mod in the real game and says what doesn't work there: what only a
//! running Horizon can tell. Through `horizon-ctl` (the game's control socket, the
//! build's own copy) it opens a creative test world, `porter-check`, in the instance
//! the mod is installed in, then:
//!
//! 1. reads the game's log: Horizon refuses every content mod when one file is
//!    wrong ("content mods not loaded"), and then nothing else is worth checking;
//! 2. places each of the mod's blocks on a pad and reads it back;
//! 3. right-clicks each block whose Java acts on a click (runtime.rs's `*_USE`
//!    lists) and reads it again;
//! 4. checks the game still answers after each step.
//!
//! It uses a control socket of its own, so a game the player has open is left alone.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::Duration;

use anyhow::{Context, Result, bail};
use serde::Serialize;
use serde_json::Value;

use crate::Instance;

/// What a play check found.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct PlayReport {
    /// The mod's blocks, and how many placed and read back as themselves.
    pub blocks: usize,
    pub placed: usize,
    /// Its blocks that act on a right click, and how many changed when clicked.
    pub acting: usize,
    pub responded: usize,
    /// What is broken: a block the game refuses, a game that stops, content Horizon
    /// didn't load.
    pub faults: Vec<String>,
    /// What may be: a click that changed nothing (some need an item in hand, or a
    /// neighbour), a block that couldn't be reached.
    pub notes: Vec<String>,
}

impl PlayReport {
    /// One line a player reads: `301 blocks place, 58 of 60 respond to a click`.
    pub fn summary(&self) -> String {
        let mut s = format!("{} of {} blocks place", self.placed, self.blocks);
        if self.acting > 0 {
            s.push_str(&format!(
                ", {} of {} respond to a click",
                self.responded, self.acting
            ));
        }
        s
    }
}

/// How to run a check.
#[derive(Debug, Clone, Default)]
pub struct PlayOptions {
    /// At most this many blocks (0: all).
    pub limit: usize,
    /// Leave the game running afterwards.
    pub keep: bool,
}

/// Plays the port in `port_dir` (the folder `port` wrote) in `instance`, where it
/// must already be installed and built.
pub fn play_check(
    instance: &Instance,
    port_dir: &Path,
    options: &PlayOptions,
    mut progress: impl FnMut(&str),
) -> Result<PlayReport> {
    let home = instance
        .home
        .clone()
        .context("the launcher didn't say where this instance's game folder is")?;
    let ctl = horizon_ctl(instance.version.as_deref())?;
    let (mut blocks, acting) = mod_blocks(port_dir);
    if options.limit > 0 {
        blocks.truncate(options.limit);
    }
    let mut report = PlayReport {
        blocks: blocks.len(),
        acting: blocks.iter().filter(|b| acting.contains(b)).count(),
        ..PlayReport::default()
    };
    if blocks.is_empty() {
        return Ok(report);
    }
    let game = Game::new(ctl, home);
    progress("Starting the game in a test world");
    // A check's game from before (still closing) would take these commands.
    if game.alive() {
        game.run(&["quit"], 60);
        for _ in 0..60 {
            if !game.alive() {
                break;
            }
            std::thread::sleep(Duration::from_secs(1));
        }
    }
    let launched = game.run(
        &["launch", "--world", "porter-check", "--creative", "--wait"],
        300,
    );
    let Some(start) = launched["player"]["position"].as_array().cloned() else {
        bail!(
            "the game didn't start: {}",
            launched["error"].as_str().unwrap_or("no answer")
        );
    };
    let at = |i: usize| start.get(i).and_then(Value::as_f64).unwrap_or(0.0).floor() as i32;
    let (px, mut py, pz) = (at(0), at(1), at(2));

    // Horizon refuses every content mod when one is wrong: nothing to place then.
    let namespaces: Vec<String> = {
        let mut ns: Vec<String> = blocks
            .iter()
            .filter_map(|b| b.split_once(':').map(|(n, _)| n.to_string()))
            .collect();
        ns.dedup();
        ns
    };
    let started = read_log(&game.home);
    if let Some(line) = started
        .iter()
        .find(|l| l.contains("content mods not loaded"))
    {
        report.faults.push(format!(
            "Horizon didn't load the content mods: {}",
            after(line, "content mods not loaded")
        ));
        game.finish(options.keep);
        return Ok(report);
    }
    // What the game couldn't draw of the mod's: a blockstate it didn't read leaves
    // its block invisible; a missing model or texture draws Minecraft's checkers.
    log_assets(&started, &namespaces, &mut report);

    // Pads on the surface where the player stands, two blocks apart, in rows.
    for dy in 0..40 {
        let below = game.state([px, py - 1 - dy, pz]);
        if !below.is_empty() && below != "minecraft:air" {
            py -= dy;
            break;
        }
    }
    // A flat stone floor with room above it, over the whole grid and where the
    // player stands, so the player can walk to every block whatever the terrain.
    let rows = blocks.len().div_ceil(16) as i32;
    let (x1, x2, z1) = (px - 2, px + 3 + 16 * 2 + 2, pz - 2);
    let z2 = pz + 3 + rows * 3 + 3;
    let mut z = z1;
    while z <= z2 {
        // A slab of rows at a time: fill takes a box of limited size.
        let end = (z + 15).min(z2);
        for (y1, y2, block) in [(py - 1, py - 1, "stone"), (py, py + 4, "air")] {
            game.run(
                &[
                    "fill",
                    &x1.to_string(),
                    &y1.to_string(),
                    &z.to_string(),
                    &x2.to_string(),
                    &y2.to_string(),
                    &end.to_string(),
                    block,
                ],
                60,
            );
        }
        z = end + 1;
    }
    let mut spots = Vec::new();
    for (i, block) in blocks.iter().enumerate() {
        let pos = [
            px + 3 + (i % 16) as i32 * 2,
            py,
            pz + 3 + (i / 16) as i32 * 3,
        ];
        let [x, y, z] = pos;
        let placed = game.set(x, y, z, block);
        if placed["ok"] == Value::Bool(false) {
            report.faults.push(format!(
                "{block}: the game refused it ({})",
                placed["error"].as_str().unwrap_or("no reason given")
            ));
            continue;
        }
        let mut got = game.state(pos);
        if !is_block(&got, block) {
            std::thread::sleep(Duration::from_millis(500));
            got = game.state(pos);
        }
        if !is_block(&got, block) {
            report.faults.push(format!(
                "{block}: placed, but the game holds {}",
                if got.is_empty() { "nothing" } else { &got }
            ));
            continue;
        }
        report.placed += 1;
        // A door is two blocks; its code reads the other half.
        if got.contains("half=lower") {
            game.set(x, y + 1, z, &got.replace("half=lower", "half=upper"));
        }
        spots.push((block.clone(), pos));
        if (i + 1) % 25 == 0 {
            progress(&format!("Placed {} of {} blocks", i + 1, blocks.len()));
            if !game.alive() {
                report
                    .faults
                    .push(format!("the game stopped after placing {block}"));
                return Ok(report);
            }
        }
    }

    let clicks: Vec<_> = spots.iter().filter(|(b, _)| acting.contains(b)).collect();
    for (n, (block, pos)) in clicks.iter().enumerate() {
        progress(&format!(
            "Right-clicking {} of {}: {block}",
            n + 1,
            clicks.len()
        ));
        let [x, y, z] = *pos;
        game.run(
            &[
                "goto",
                &(f64::from(x) + 0.5).to_string(),
                &y.to_string(),
                &(f64::from(z + 2) + 0.5).to_string(),
                "--direct",
                "--timeout",
                "15",
            ],
            30,
        );
        let before = game.state(*pos);
        let mut clicked = game.interact(*pos, "south");
        if clicked["ok"] == Value::Bool(false) {
            clicked = game.interact(*pos, "up");
        }
        std::thread::sleep(Duration::from_secs(1));
        if !game.alive() {
            report
                .faults
                .push(format!("{block}: the game stopped after a right click"));
            return Ok(report);
        }
        let mut after = game.state(*pos);
        if before == after && clicked["ok"] != Value::Bool(false) {
            // The click may have met another block first: once more, from above.
            game.interact(*pos, "up");
            std::thread::sleep(Duration::from_secs(1));
            after = game.state(*pos);
        }
        if clicked["ok"] == Value::Bool(false) {
            report.notes.push(format!(
                "{block}: couldn't be clicked ({})",
                clicked["error"].as_str().unwrap_or("not reached")
            ));
        } else if before == after {
            report
                .notes
                .push(format!("{block}: a right click changed nothing"));
        } else {
            report.responded += 1;
        }
    }
    for line in read_log(&game.home)
        .iter()
        .filter(|l| l.contains("panicked") || l.contains("/ERROR]"))
        .take(5)
    {
        report.faults.push(format!(
            "the game's log: {}",
            line.chars().take(200).collect::<String>()
        ));
    }
    game.finish(options.keep);
    Ok(report)
}

/// The lines of the instance's game log (`logs/latest.log`).
fn read_log(home: &Path) -> Vec<String> {
    std::fs::read_to_string(home.join("logs").join("latest.log"))
        .unwrap_or_default()
        .lines()
        .map(str::to_string)
        .collect()
}

/// What follows `marker` in `line`, past its `: `.
fn after<'a>(line: &'a str, marker: &str) -> &'a str {
    line.find(marker)
        .map_or(line, |i| &line[i + marker.len()..])
        .trim_start_matches(':')
        .trim()
}

/// The game's warnings about the mod's assets, as faults (a block it can't draw)
/// and notes (a model or texture it draws as Minecraft's missing one).
fn log_assets(log: &[String], namespaces: &[String], report: &mut PlayReport) {
    let ours = |text: &str| namespaces.iter().any(|ns| text.contains(&format!("{ns}:")));
    let mut models = Vec::new();
    let mut textures = Vec::new();
    for line in log.iter().filter(|l| ours(l)) {
        if let Some(rest) = line.split("blockstate definition of ").nth(1) {
            let (block, why) = rest.split_once(" not loaded").unwrap_or((rest, ""));
            let why = why.trim_start_matches(':').trim();
            let why = if why.is_empty() {
                String::new()
            } else {
                format!(" ({why})")
            };
            report.faults.push(format!(
                "{block}: draws nothing, its blockstate file didn't load{why}"
            ));
        } else if let Some(rest) = line.split("missing block model ").nth(1) {
            models.push(rest.split(':').take(2).collect::<Vec<_>>().join(":"));
        } else if let Some(rest) = line.split("missing texture for ").nth(1) {
            textures.push(rest.split(':').take(2).collect::<Vec<_>>().join(":"));
        }
    }
    for (what, list) in [("model", models), ("texture", textures)] {
        if list.is_empty() {
            continue;
        }
        let shown: Vec<&str> = list.iter().take(5).map(String::as_str).collect();
        report.notes.push(format!(
            "{} missing {what}{}, drawn as the missing {what}: {}{}",
            list.len(),
            if list.len() == 1 { "" } else { "s" },
            shown.join(", "),
            if list.len() > shown.len() {
                ", ..."
            } else {
                ""
            }
        ));
    }
}

/// Whether a block state as the game prints it (`mod:door[facing=north]`) is `block`'s.
fn is_block(state: &str, block: &str) -> bool {
    state.split('[').next() == Some(block)
}

/// The content mod's blocks (sorted ids), and those whose Java acts on a right click.
pub fn mod_blocks(port_dir: &Path) -> (Vec<String>, Vec<String>) {
    let mut blocks = Vec::new();
    if let Ok(namespaces) = std::fs::read_dir(port_dir.join("content").join("data")) {
        for ns in namespaces.flatten() {
            let dir = ns.path().join("horizon").join("block");
            for f in std::fs::read_dir(dir).into_iter().flatten().flatten() {
                let path = f.path();
                if path.extension().is_some_and(|e| e == "json")
                    && let Some(stem) = path.file_stem()
                {
                    blocks.push(format!(
                        "{}:{}",
                        ns.file_name().to_string_lossy(),
                        stem.to_string_lossy()
                    ));
                }
            }
        }
    }
    blocks.sort();
    let mut acting = Vec::new();
    for bundle in std::fs::read_dir(port_dir.join("mixins"))
        .into_iter()
        .flatten()
        .flatten()
    {
        let Ok(text) = std::fs::read_to_string(bundle.path().join("runtime.rs")) else {
            continue;
        };
        let mut inside = false;
        for line in text.lines() {
            let t = line.trim();
            if t.starts_with("const ") && t.contains("_USE: &[&str] = &[") {
                inside = true;
                continue;
            }
            if inside {
                if t.starts_with("];") {
                    inside = false;
                } else if let Some(id) = t.strip_prefix('"').and_then(|r| r.split('"').next()) {
                    acting.push(id.to_string());
                }
            }
        }
    }
    acting.sort();
    acting.dedup();
    (blocks, acting)
}

/// The `horizon-ctl` of the version an instance plays (its native build), else of
/// the newest version installed.
fn horizon_ctl(version: Option<&str>) -> Result<PathBuf> {
    let exe = if cfg!(windows) {
        "horizon-ctl.exe"
    } else {
        "horizon-ctl"
    };
    let versions = launcher_data()?.join("versions");
    if let Some(v) = version {
        let ctl = versions.join(v).join(exe);
        if ctl.is_file() {
            return Ok(ctl);
        }
    }
    let mut found: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&versions)
        .with_context(|| format!("reading {}", versions.display()))?
        .flatten()
        .map(|e| e.path().join(exe))
        .filter(|p| p.is_file())
        .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
        .collect();
    found.sort();
    found
        .pop()
        .map(|(_, p)| p)
        .context("no Horizon version with horizon-ctl: build the instance first")
}

/// The Horizon launcher's data folder, where its versions are.
fn launcher_data() -> Result<PathBuf> {
    if let Some(dir) = std::env::var_os("HORIZON_LAUNCHER_DATA") {
        return Ok(PathBuf::from(dir));
    }
    let base = if cfg!(windows) {
        std::env::var_os("APPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Application Support"))
    } else {
        std::env::var_os("XDG_DATA_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")))
    };
    Ok(base
        .context("can't tell where the Horizon launcher keeps its data")?
        .join("horizon-launcher"))
}

/// The game, through `horizon-ctl` and a control socket of the check's own.
struct Game {
    ctl: PathBuf,
    home: PathBuf,
    socket: PathBuf,
}

impl Game {
    fn new(ctl: PathBuf, home: PathBuf) -> Game {
        let socket = std::env::temp_dir().join("horizon-porter-check.json");
        Game { ctl, home, socket }
    }

    /// Runs one command; its JSON answer, or `{"ok": false, "error": ..}`.
    fn run(&self, args: &[&str], timeout: u64) -> Value {
        let out = Command::new(&self.ctl)
            .arg("--socket")
            .arg(&self.socket)
            .args(["--timeout", &timeout.to_string()])
            .args(args)
            .env("HORIZON_HOME", &self.home)
            .stdin(Stdio::null())
            .output();
        let Ok(out) = out else {
            return serde_json::json!({ "ok": false, "error": "horizon-ctl didn't run" });
        };
        let text = String::from_utf8_lossy(&out.stdout);
        text.lines()
            .rev()
            .find_map(|l| serde_json::from_str(l).ok())
            .unwrap_or_else(|| {
                serde_json::json!({
                    "ok": false,
                    "error": String::from_utf8_lossy(&out.stderr).trim().to_string()
                })
            })
    }

    fn alive(&self) -> bool {
        self.run(&["hello"], 15).get("protocol").is_some()
    }

    fn state(&self, [x, y, z]: [i32; 3]) -> String {
        self.run(
            &["block", &x.to_string(), &y.to_string(), &z.to_string()],
            30,
        )["state"]
            .as_str()
            .unwrap_or_default()
            .to_string()
    }

    fn set(&self, x: i32, y: i32, z: i32, block: &str) -> Value {
        self.run(
            &[
                "setblock",
                &x.to_string(),
                &y.to_string(),
                &z.to_string(),
                block,
            ],
            30,
        )
    }

    fn interact(&self, [x, y, z]: [i32; 3], face: &str) -> Value {
        self.run(
            &[
                "interact",
                &x.to_string(),
                &y.to_string(),
                &z.to_string(),
                face,
            ],
            30,
        )
    }

    fn finish(&self, keep: bool) {
        if !keep {
            self.run(&["quit"], 60);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_the_blocks_and_those_that_act() {
        let dir = std::env::temp_dir().join(format!("porter-play-{}", std::process::id()));
        let blocks = dir.join("content/data/shutters/horizon/block");
        std::fs::create_dir_all(&blocks).unwrap();
        std::fs::write(blocks.join("oak_shutter.json"), "{}").unwrap();
        std::fs::write(blocks.join("iron_gate.json"), "{}").unwrap();
        std::fs::create_dir_all(dir.join("mixins/shutters")).unwrap();
        std::fs::write(
            dir.join("mixins/shutters/runtime.rs"),
            "const SHUTTER_USE: &[&str] = &[\n    \"shutters:oak_shutter\",\n];\nconst SHUTTER_PLACE: &[&str] = &[\n    \"shutters:iron_gate\",\n];\n",
        )
        .unwrap();
        let (all, acting) = mod_blocks(&dir);
        assert_eq!(all, ["shutters:iron_gate", "shutters:oak_shutter"]);
        assert_eq!(acting, ["shutters:oak_shutter"]);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn reads_what_the_game_couldnt_draw() {
        let log: Vec<String> = [
            "[20:27:48] [load-render/WARN]: Horizon: blockstate definition of mcwwindows:poplar_log_parapet not loaded: unknown multipart property",
            "[20:27:48] [load-render/WARN]: Horizon: missing block model mcwdoors:block/waffle/cherry_waffle_door_top: The system cannot find the file specified. (os error 2)",
            "[20:27:48] [load-render/WARN]: Horizon: using the missing texture for biomesoplenty:block/bush: The system cannot find the file specified.",
            "[20:27:48] [load-render/WARN]: Horizon: missing block model othermod:block/thing: nope",
        ]
        .map(String::from)
        .to_vec();
        let mut r = PlayReport::default();
        log_assets(&log, &["mcwwindows".into(), "mcwdoors".into()], &mut r);
        assert_eq!(
            r.faults,
            [
                "mcwwindows:poplar_log_parapet: draws nothing, its blockstate file didn't load (unknown multipart property)"
            ]
        );
        assert_eq!(
            r.notes,
            [
                "1 missing model, drawn as the missing model: mcwdoors:block/waffle/cherry_waffle_door_top"
            ]
        );
        assert_eq!(
            after(
                "x: content mods not loaded: data/a.json: bad",
                "content mods not loaded"
            ),
            "data/a.json: bad"
        );
    }

    #[test]
    fn says_it_in_a_line() {
        let r = PlayReport {
            blocks: 301,
            placed: 301,
            acting: 60,
            responded: 58,
            ..PlayReport::default()
        };
        assert_eq!(
            r.summary(),
            "301 of 301 blocks place, 58 of 60 respond to a click"
        );
        assert!(is_block(
            "mcwdoors:garage_white_door[open=true]",
            "mcwdoors:garage_white_door"
        ));
    }
}
