//! Builds ported mods with the player's installed Horizon launcher.
//!
//! [`Launcher`] finds `horizon-launcher`, lists and creates instances, and runs
//! `native-build` on a bundle folder with live progress. Native mods are not
//! sandboxed, so a build needs a [`Trust`]: only the player's explicit choice
//! creates one (Horizon's rule: trusting native code is the player's decision).

pub mod play;

use std::ffi::OsStr;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use serde::Deserialize;

/// The player's decision to trust a bundle's native code with full access to their
/// computer. Create it only from that decision: a flag they typed or a button they
/// pressed after reading what it means, never by default.
pub struct Trust(());

impl Trust {
    /// The player chose to trust this code.
    pub fn given_by_player() -> Trust {
        Trust(())
    }
}

/// An instance, as `horizon-launcher instance list --json` lists it.
#[derive(Debug, Clone, Deserialize)]
pub struct Instance {
    pub id: String,
    pub name: String,
    /// The version it plays, such as `native-6f8c7a0-…` after a native build.
    #[serde(default)]
    pub version: Option<String>,
    #[serde(default)]
    pub selected: bool,
    /// Its game folder, where `horizon-mods/` and `horizon-native-mods/` are.
    #[serde(default)]
    pub home: Option<PathBuf>,
    /// `default`, `own`, or `java` for a Java Edition instance.
    #[serde(default)]
    pub kind: Option<String>,
}

impl Instance {
    /// Whether it plays Horizon (not Java Edition), so Horizon mods go in it.
    pub fn plays_horizon(&self) -> bool {
        self.kind.as_deref() != Some("java")
    }
}

/// What [`install`] put in an instance.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Installed {
    /// The content mod, in `horizon-mods/`.
    pub content: Option<PathBuf>,
    /// The native mod, in `horizon-native-mods/`: the instance needs a native build.
    pub native: Option<PathBuf>,
}

/// Installs a ported mod's packaged files into a Horizon instance's game folder
/// `home`, beside its other mods: the content mod (`.hmod`) in `horizon-mods/`, the
/// native mod (`.hnmod`) in `horizon-native-mods/`, as the launcher keeps them, and
/// turns it on in `horizon-mods.txt` (Options > Mods' switch). Building the instance
/// is the player's step: [`Launcher::build_instance`], or the launcher's own Mods page.
pub fn install(
    home: &Path,
    id: &str,
    hmod: Option<&Path>,
    hnmod: Option<&Path>,
) -> Result<Installed> {
    if id.is_empty() || !id.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
        bail!("{id:?} is no mod id");
    }
    let mut installed = Installed {
        content: None,
        native: None,
    };
    let place = |from: &Path, dir: &str, ext: &str| -> Result<PathBuf> {
        let folder = home.join(dir);
        std::fs::create_dir_all(&folder)
            .with_context(|| format!("creating {}", folder.display()))?;
        let to = folder.join(format!("{id}.{ext}"));
        std::fs::copy(from, &to)
            .with_context(|| format!("copying {} to {}", from.display(), to.display()))?;
        Ok(to)
    };
    if let Some(f) = hmod {
        installed.content = Some(place(f, "horizon-mods", "hmod")?);
    }
    if let Some(f) = hnmod {
        installed.native = Some(place(f, "horizon-native-mods", "hnmod")?);
    }
    turn_on(home, id)?;
    Ok(installed)
}

/// Sets `mod.<id>:true` in the instance's `horizon-mods.txt`, keeping its other lines.
fn turn_on(home: &Path, id: &str) -> Result<()> {
    let file = home.join("horizon-mods.txt");
    let text = std::fs::read_to_string(&file).unwrap_or_default();
    let key = format!("mod.{id}:");
    let mut lines: Vec<String> = text
        .lines()
        .filter(|l| !l.trim_start().starts_with(&key))
        .map(str::to_string)
        .collect();
    lines.push(format!("mod.{id}:true"));
    std::fs::write(&file, lines.join("\n") + "\n")
        .with_context(|| format!("writing {}", file.display()))
}

/// The installed Horizon launcher.
#[derive(Debug, Clone)]
pub struct Launcher {
    pub path: PathBuf,
    /// `horizon-launcher 0.2.40`.
    pub version: String,
}

impl Launcher {
    /// Finds the launcher: `explicit`, then `HORIZON_LAUNCHER`, then `PATH`, then the
    /// places it usually lives (including where Windows unpacks it when it is run
    /// straight from its zip). Each candidate must answer `--version`.
    pub fn find(explicit: Option<&Path>) -> Result<Launcher> {
        if let Some(path) = explicit {
            return Launcher::at(path);
        }
        if let Some(path) = std::env::var_os("HORIZON_LAUNCHER") {
            return Launcher::at(Path::new(&path));
        }
        for candidate in candidates() {
            if let Ok(launcher) = Launcher::at(&candidate) {
                return Ok(launcher);
            }
        }
        bail!(
            "could not find the Horizon launcher. Install it from https://playhorizon.dev, or say where it is with --launcher PATH (or the HORIZON_LAUNCHER variable)"
        )
    }

    /// The launcher at `path`, if it answers `--version` as horizon-launcher.
    pub fn at(path: &Path) -> Result<Launcher> {
        let output = command(path)
            .arg("--version")
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running {}", path.display()))?;
        let version = String::from_utf8_lossy(&output.stdout).trim().to_string();
        if !output.status.success() || !version.starts_with("horizon-launcher") {
            bail!("{} is not the Horizon launcher", path.display());
        }
        Ok(Launcher {
            path: path.to_path_buf(),
            version,
        })
    }

    pub fn instances(&self) -> Result<Vec<Instance>> {
        let output = self.run(["instance", "list", "--json"])?;
        serde_json::from_str(&output).context("reading the launcher's instance list")
    }

    /// The instance named `name` (any case), created first when there is none.
    pub fn ensure_instance(&self, name: &str) -> Result<Instance> {
        let find = |list: Vec<Instance>| {
            list.into_iter()
                .find(|i| i.name.eq_ignore_ascii_case(name) || i.id.eq_ignore_ascii_case(name))
        };
        if let Some(instance) = find(self.instances()?) {
            return Ok(instance);
        }
        self.run(["instance", "create", name])?;
        find(self.instances()?)
            .with_context(|| format!("the launcher did not create instance {name}"))
    }

    /// Compiles `folder` (an unpacked .hnmod or a bundle) into `instance`'s game with
    /// `native-build`, calling `progress` with each line the launcher prints
    /// (`[ 3:47] Compiling the game: 432 of 446 crates`). Returns its last line,
    /// such as `native-… built and selected for its instance (515 s).`
    pub fn native_build(
        &self,
        folder: &Path,
        instance: &Instance,
        trust: Trust,
        progress: impl FnMut(&str),
    ) -> Result<String> {
        self.run_native_build(Some(folder), instance, &trust, progress)
    }

    /// Builds every native mod turned on in `instance` (its `horizon-native-mods/`),
    /// together: what an instance needs after [`install`] adds one.
    pub fn build_instance(
        &self,
        instance: &Instance,
        trust: Trust,
        progress: impl FnMut(&str),
    ) -> Result<String> {
        self.run_native_build(None, instance, &trust, progress)
    }

    /// [`build_instance`], and when the build fails, `repair` gets its log
    /// (native/build.log) and the Horizon source it compiled: it changes the mods
    /// (putting back code the build refused, installing them again) and says what
    /// it changed, and the build runs again; at most three times. Nothing changed
    /// leaves the build's error. Returns the build's last line and what was changed.
    pub fn build_instance_repairing(
        &self,
        instance: &Instance,
        trust: Trust,
        mut repair: impl FnMut(&str, &Path) -> Result<Vec<String>>,
        mut progress: impl FnMut(&str),
    ) -> Result<(String, Vec<String>)> {
        let mut changed = Vec::new();
        for round in 0.. {
            let err = match self.run_native_build(None, instance, &trust, &mut progress) {
                Ok(last) => return Ok((last, changed)),
                Err(e) => e,
            };
            if round == 3 {
                return Err(err);
            }
            let native = play::launcher_data()?.join("native");
            let log = std::fs::read_to_string(native.join("build.log")).unwrap_or_default();
            // The tree it compiled: the newest of the patched copies of Horizon.
            let tree = std::fs::read_dir(native.join("patched"))
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
                .max()
                .map(|(_, p)| p);
            let Some(tree) = tree else {
                return Err(err);
            };
            let done = repair(&log, &tree)?;
            if done.is_empty() {
                return Err(err);
            }
            for d in &done {
                progress(&format!("Put back to its stub, the build refused it: {d}"));
            }
            changed.extend(done);
        }
        unreachable!()
    }

    fn run_native_build(
        &self,
        folder: Option<&Path>,
        instance: &Instance,
        _trust: &Trust,
        mut progress: impl FnMut(&str),
    ) -> Result<String> {
        let mut cmd = command(&self.path);
        cmd.arg("native-build");
        if let Some(folder) = folder {
            cmd.arg(folder);
        }
        let mut child = cmd
            .args(["--instance", &instance.id, "--trust-native-code"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .spawn()
            .with_context(|| format!("starting {}", self.path.display()))?;
        // Progress comes on stderr, the result on stdout: read stdout on its own thread.
        let mut stdout = child.stdout.take().context("no stdout")?;
        let out_thread = std::thread::spawn(move || {
            let mut text = String::new();
            let _ = stdout.read_to_string(&mut text);
            text
        });
        let mut tail: Vec<String> = Vec::new();
        if let Some(stderr) = child.stderr.take() {
            for line in BufReader::new(stderr).lines().map_while(Result::ok) {
                let line = line.trim_end().to_string();
                if line.is_empty() {
                    continue;
                }
                progress(&line);
                tail.push(line);
                if tail.len() > 30 {
                    tail.remove(0);
                }
            }
        }
        let status = child.wait()?;
        let stdout = out_thread.join().unwrap_or_default();
        for line in stdout.lines().filter(|l| !l.trim().is_empty()) {
            progress(line.trim_end());
            tail.push(line.trim_end().to_string());
        }
        if !status.success() {
            bail!(
                "the build failed:\n{}\n(the full log is native/build.log in the launcher's data folder)",
                tail.join("\n")
            );
        }
        Ok(tail.last().cloned().unwrap_or_default())
    }

    /// Starts `instance` in the background (`play --instance`); the launcher plays it
    /// with the signed-in account.
    /// Starts the game in `instance`, with each of `dev_mods` (a content mod folder or
    /// `.hmod`) loaded and turned on, as `cargo horizon-mod run` does.
    pub fn play(&self, instance: &Instance, dev_mods: &[&Path]) -> Result<()> {
        let mut cmd = command(&self.path);
        cmd.args(["play", "--instance", &instance.id]);
        for m in dev_mods {
            // The game runs in the instance's folder: a relative path would point
            // there, and the content mod would silently not load.
            let full =
                std::path::absolute(m).with_context(|| format!("finding {}", m.display()))?;
            cmd.args(["--arg", "--dev-mod", "--arg"]).arg(full);
        }
        cmd.stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .with_context(|| format!("starting {}", self.path.display()))?;
        Ok(())
    }

    fn run<I, S>(&self, args: I) -> Result<String>
    where
        I: IntoIterator<Item = S>,
        S: AsRef<OsStr>,
    {
        let output = command(&self.path)
            .args(args)
            .stdin(Stdio::null())
            .output()
            .with_context(|| format!("running {}", self.path.display()))?;
        if !output.status.success() {
            bail!(
                "the launcher said: {}",
                String::from_utf8_lossy(&output.stderr).trim()
            );
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }
}

/// The folder to give `native-build` for a bundle `port` wrote: the folder itself
/// when it is an unpacked .hnmod (it has horizon-native.json, so the build uses the
/// Horizon the player plays), else its single `mixins/<id>/` bundle.
pub fn build_folder(dir: &Path) -> Result<PathBuf> {
    if dir.join("horizon-native.json").is_file() || dir.join("horizon-mixin.toml").is_file() {
        return Ok(dir.to_path_buf());
    }
    let bundles: Vec<PathBuf> = std::fs::read_dir(dir.join("mixins"))
        .with_context(|| format!("{} is not a bundle folder `port` wrote", dir.display()))?
        .filter_map(|e| e.ok().map(|e| e.path()))
        .filter(|p| p.join("horizon-mixin.toml").is_file())
        .collect();
    match bundles.as_slice() {
        [one] => Ok(one.clone()),
        _ => bail!("{} should hold one bundle under mixins/", dir.display()),
    }
}

/// A folder `port` wrote: its native half, its content half, or both.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Port {
    pub id: String,
    /// The folder to give `native-build`, when it has a native half.
    pub native: Option<PathBuf>,
    /// The content mod folder (`content/`), when it has one.
    pub content: Option<PathBuf>,
}

impl Port {
    pub fn read(dir: &Path) -> Result<Port> {
        let content_dir = dir.join("content");
        let content = content_dir
            .join("horizon-mod.toml")
            .is_file()
            .then_some(content_dir);
        let native = if dir.join("mixins").is_dir() || dir.join("horizon-native.json").is_file() {
            Some(build_folder(dir)?)
        } else {
            None
        };
        let id = if let Some(folder) = &native {
            let bundle = if folder == dir {
                std::fs::read_dir(dir.join("mixins"))?
                    .filter_map(|e| e.ok().map(|e| e.path()))
                    .find(|p| p.join("horizon-mixin.toml").is_file())
                    .with_context(|| format!("{} has no bundle under mixins/", dir.display()))?
            } else {
                folder.clone()
            };
            bundle
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default()
        } else if let Some(content) = &content {
            let text = std::fs::read_to_string(content.join("horizon-mod.toml"))?;
            text.lines()
                .filter_map(|l| l.trim().strip_prefix("id"))
                .filter_map(|l| l.trim_start().strip_prefix('='))
                .map(|v| v.trim().trim_matches('"').to_string())
                .next()
                .context("content/horizon-mod.toml has no id")?
        } else {
            bail!(
                "{} is not a folder `port` wrote: it has neither mixins/ nor content/",
                dir.display()
            );
        };
        Ok(Port {
            id,
            native,
            content,
        })
    }
}

/// A command for the launcher that opens no console window on Windows.
fn command(path: &Path) -> Command {
    #[allow(unused_mut)]
    let mut cmd = Command::new(path);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}

/// Where the launcher usually is, most likely first.
fn candidates() -> Vec<PathBuf> {
    let exe = if cfg!(windows) {
        "horizon-launcher.exe"
    } else {
        "horizon-launcher"
    };
    let mut out = Vec::new();
    if let Some(path) = std::env::var_os("PATH") {
        out.extend(std::env::split_paths(&path).map(|d| d.join(exe)));
    }
    let home =
        std::env::var_os(if cfg!(windows) { "USERPROFILE" } else { "HOME" }).map(PathBuf::from);
    if cfg!(windows) {
        if let Some(local) = std::env::var_os("LOCALAPPDATA").map(PathBuf::from) {
            for dir in [
                "Programs\\Horizon",
                "Programs\\horizon-launcher",
                "Programs\\Horizon Launcher",
                "horizon-launcher",
            ] {
                out.push(local.join(dir).join(exe));
            }
        }
        if let Some(home) = &home {
            out.push(
                home.join("Downloads")
                    .join("horizon-launcher-windows-x64")
                    .join(exe),
            );
            out.push(home.join("Desktop").join(exe));
        }
        // Run straight from its zip, Windows unpacks it to a temporary folder.
        if let Some(temp) = std::env::var_os("TEMP").map(PathBuf::from) {
            let mut unpacked: Vec<(std::time::SystemTime, PathBuf)> = std::fs::read_dir(&temp)
                .into_iter()
                .flatten()
                .filter_map(|e| e.ok())
                .filter(|e| e.file_name().to_string_lossy().contains("horizon-launcher"))
                .map(|e| e.path().join(exe))
                .filter(|p| p.is_file())
                .filter_map(|p| Some((p.metadata().ok()?.modified().ok()?, p)))
                .collect();
            unpacked.sort();
            out.extend(unpacked.into_iter().rev().map(|(_, p)| p));
        }
    } else if cfg!(target_os = "macos") {
        for app in ["Horizon.app", "Horizon Launcher.app"] {
            out.push(
                PathBuf::from("/Applications")
                    .join(app)
                    .join("Contents/MacOS")
                    .join(exe),
            );
            if let Some(home) = &home {
                out.push(
                    home.join("Applications")
                        .join(app)
                        .join("Contents/MacOS")
                        .join(exe),
                );
            }
        }
    } else if let Some(home) = &home {
        out.push(home.join(".local/bin").join(exe));
        out.push(PathBuf::from("/usr/local/bin").join(exe));
        out.push(PathBuf::from("/usr/bin").join(exe));
    }
    out
}

#[cfg(test)]
mod install_tests {
    use super::*;

    #[test]
    fn installs_beside_other_mods_and_turns_it_on() {
        let home = std::env::temp_dir().join(format!("porter-install-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&home);
        std::fs::create_dir_all(&home).unwrap();
        std::fs::write(
            home.join("horizon-mods.txt"),
            "mod.other:true\nmod.demo:false\n",
        )
        .unwrap();
        let src = home.join("src");
        std::fs::create_dir_all(&src).unwrap();
        std::fs::write(src.join("demo-1.0.hmod"), "content").unwrap();
        std::fs::write(src.join("demo-1.0.hnmod"), "native").unwrap();
        let got = install(
            &home,
            "demo",
            Some(&src.join("demo-1.0.hmod")),
            Some(&src.join("demo-1.0.hnmod")),
        )
        .unwrap();
        assert_eq!(
            got.content,
            Some(home.join("horizon-mods").join("demo.hmod"))
        );
        assert_eq!(
            got.native,
            Some(home.join("horizon-native-mods").join("demo.hnmod"))
        );
        assert_eq!(
            std::fs::read_to_string(home.join("horizon-mods.txt")).unwrap(),
            "mod.other:true\nmod.demo:true\n"
        );
        assert!(install(&home, "../x", None, None).is_err());
        std::fs::remove_dir_all(&home).ok();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_instance_lists() {
        let list: Vec<Instance> = serde_json::from_str(
            r#"[{"id":"horizon","name":"Horizon","kind":"default","version":null,"selected":false},
                {"id":"porter-test","name":"Porter test","version":"native-6f8c7a0-5065a705b860","selected":true}]"#,
        )
        .unwrap();
        assert_eq!(list[1].id, "porter-test");
        assert_eq!(
            list[1].version.as_deref(),
            Some("native-6f8c7a0-5065a705b860")
        );
        assert!(list[1].selected);
    }

    #[test]
    fn finds_the_folder_to_build() {
        let dir = std::env::temp_dir().join(format!("porter-verify-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("mixins/demo")).unwrap();
        std::fs::write(dir.join("mixins/demo/horizon-mixin.toml"), "").unwrap();
        // A bundle without horizon-native.json builds from its mixins/<id>/ folder...
        assert_eq!(build_folder(&dir).unwrap(), dir.join("mixins/demo"));
        // ...one with it builds as an unpacked .hnmod.
        std::fs::write(dir.join("horizon-native.json"), "{}").unwrap();
        assert_eq!(build_folder(&dir).unwrap(), dir);
        std::fs::remove_dir_all(&dir).ok();
    }

    /// Only meaningful where a launcher is installed: `cargo test -- --ignored`.
    #[test]
    #[ignore]
    fn finds_an_installed_launcher() {
        let launcher = Launcher::find(None).unwrap();
        println!("found {} at {}", launcher.version, launcher.path.display());
        let instances = launcher.instances().unwrap();
        println!(
            "instances: {:?}",
            instances.iter().map(|i| &i.name).collect::<Vec<_>>()
        );
        assert!(!instances.is_empty());
    }

    #[test]
    fn a_missing_launcher_is_explained() {
        let err = Launcher::at(Path::new("no/such/horizon-launcher")).unwrap_err();
        assert!(format!("{err:#}").contains("running"));
    }
}
