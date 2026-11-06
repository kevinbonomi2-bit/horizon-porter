//! Turns a mod's jar back into a source tree, so it can be ported like a mod's own
//! source folder: its classes decompiled with Vineflower (Apache-2.0,
//! github.com/Vineflower/vineflower) into `src/main/java/`, and everything else in
//! the jar (assets, data, mod metadata, Mixin configs) into `src/main/resources/`.
//!
//! Vineflower runs on a Java runtime: the one the Horizon launcher keeps for Java
//! Edition, else `JAVA_HOME` or `java` on the PATH. It is downloaded once from Maven
//! Central and checked against the SHA-256 Maven Central publishes for it. A jar is
//! decompiled once: the tree is kept, keyed by the jar's own SHA-256.

use std::fs;
use std::io::Read;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result, bail};
use sha2::{Digest, Sha256};

pub const VINEFLOWER_VERSION: &str = "1.12.0";
/// Maven Central's published SHA-256 of vineflower-1.12.0.jar.
const VINEFLOWER_SHA256: &str = "1dfcfe974395734fa467ce620661c7623d05ba83670de0529b1fbd63ff548b9d";
const VINEFLOWER_URL: &str =
    "https://repo1.maven.org/maven2/org/vineflower/vineflower/1.12.0/vineflower-1.12.0.jar";

/// Where horizon-porter keeps what it downloads and decompiles:
/// `HORIZON_PORTER_CACHE`, else the system's local data or cache folder.
pub fn cache_dir() -> PathBuf {
    if let Some(dir) = std::env::var_os("HORIZON_PORTER_CACHE") {
        return PathBuf::from(dir);
    }
    if let Some(dir) = std::env::var_os("LOCALAPPDATA") {
        return PathBuf::from(dir).join("horizon-porter");
    }
    if let Some(dir) = std::env::var_os("XDG_CACHE_HOME") {
        return PathBuf::from(dir).join("horizon-porter");
    }
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home);
        if cfg!(target_os = "macos") {
            return home.join("Library/Caches/horizon-porter");
        }
        return home.join(".cache/horizon-porter");
    }
    std::env::temp_dir().join("horizon-porter")
}

/// A Java runtime to run Vineflower: the Horizon launcher's own (it keeps one for
/// Java Edition), else `JAVA_HOME`, else `java` on the PATH.
pub fn find_java() -> Option<PathBuf> {
    let exe = if cfg!(windows) { "java.exe" } else { "java" };
    let mut data_dirs: Vec<PathBuf> = Vec::new();
    for var in ["APPDATA", "XDG_DATA_HOME"] {
        if let Some(d) = std::env::var_os(var) {
            data_dirs.push(PathBuf::from(d));
        }
    }
    if let Some(home) = std::env::var_os("HOME").map(PathBuf::from) {
        data_dirs.push(home.join(".local/share"));
        data_dirs.push(home.join("Library/Application Support"));
    }
    let mut found: Vec<PathBuf> = Vec::new();
    for d in data_dirs {
        let runtime = d.join("horizon-launcher").join("minecraft").join("runtime");
        // <runtime>/<name>/<platform>/<name>/bin/java, or deeper on macOS.
        let mut stack = vec![(runtime, 0)];
        while let Some((dir, depth)) = stack.pop() {
            let candidate = dir.join("bin").join(exe);
            if candidate.is_file() {
                found.push(candidate);
                continue;
            }
            if depth >= 6 {
                continue;
            }
            if let Ok(entries) = fs::read_dir(&dir) {
                for e in entries.flatten() {
                    if e.path().is_dir() {
                        stack.push((e.path(), depth + 1));
                    }
                }
            }
        }
    }
    found.sort();
    if let Some(newest) = found.pop() {
        return Some(newest);
    }
    if let Some(home) = std::env::var_os("JAVA_HOME") {
        let java = PathBuf::from(home).join("bin").join(exe);
        if java.is_file() {
            return Some(java);
        }
    }
    let on_path = Command::new("java")
        .arg("-version")
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|s| s.success());
    on_path.then(|| PathBuf::from("java"))
}

fn sha256_hex(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}

/// Vineflower's jar in `cache`, downloaded and checked the first time.
pub fn vineflower(cache: &Path, progress: &mut impl FnMut(&str)) -> Result<PathBuf> {
    let jar = cache.join(format!("vineflower-{VINEFLOWER_VERSION}.jar"));
    if let Ok(bytes) = fs::read(&jar)
        && sha256_hex(&bytes) == VINEFLOWER_SHA256
    {
        return Ok(jar);
    }
    progress(&format!(
        "Downloading the Vineflower decompiler {VINEFLOWER_VERSION} (1.6 MB, once)"
    ));
    let mut bytes = Vec::new();
    ureq::get(VINEFLOWER_URL)
        .call()
        .context("downloading Vineflower from Maven Central")?
        .body_mut()
        .as_reader()
        .read_to_end(&mut bytes)
        .context("downloading Vineflower")?;
    let got = sha256_hex(&bytes);
    if got != VINEFLOWER_SHA256 {
        bail!(
            "the Vineflower download is not the file Maven Central publishes (SHA-256 {got}, expected {VINEFLOWER_SHA256}): it was not used"
        );
    }
    fs::create_dir_all(cache)?;
    let tmp = jar.with_extension("part");
    fs::write(&tmp, &bytes)?;
    fs::rename(&tmp, &jar)?;
    Ok(jar)
}

/// The jar `jar` as a source tree: `<cache>/decompiled/<its SHA-256>/` with
/// `src/main/java/` and `src/main/resources/`, and a `src/<module>/` for each of the
/// mod's own modules nested in it (jar-in-jar: `c2me-base` inside `c2me`). Libraries
/// nested in it are left out. Decompiled once; later calls return the same folder.
pub fn decompile(jar: &Path, cache: &Path, mut progress: impl FnMut(&str)) -> Result<PathBuf> {
    let bytes = fs::read(jar).with_context(|| format!("reading {}", jar.display()))?;
    let key = sha256_hex(&bytes);
    let out = cache.join("decompiled").join(&key[..16]);
    if out.join(".decompiled").is_file() {
        return Ok(out);
    }
    let parent_id = mod_id(&bytes);
    // The jar and the mod's own modules nested in it: (source set, jar bytes).
    let mut parts: Vec<(String, Vec<u8>)> = vec![("main".into(), bytes.clone())];
    let mut libraries = 0;
    for (name, inner) in nested_jars(&bytes)? {
        match (&parent_id, mod_id(&inner)) {
            (Some(parent), Some(id)) if is_module_of(&id, parent) => {
                parts.push((source_set(&id, &name), inner));
            }
            _ => libraries += 1,
        }
    }
    let classes: usize = parts.iter().map(|(_, b)| class_count(b)).sum();
    // A data-only mod (structures, worldgen in a jar: Dungeons and Taverns): its
    // resources are the whole mod, ported as a content mod.
    if classes == 0 {
        let _ = fs::remove_dir_all(&out);
        for (set, part) in &parts {
            let res_dir = out.join("src").join(set).join("resources");
            fs::create_dir_all(&res_dir)?;
            unpack_resources(part, &res_dir)?;
        }
        fs::create_dir_all(out.join("src").join("main").join("java"))?;
        fs::write(
            out.join(".decompiled"),
            format!("{}\nno classes: resources only\n", jar.display()),
        )?;
        return Ok(out);
    }
    let java = find_java().context(
        "no Java runtime to decompile the jar with: open the Horizon launcher once (it keeps one), or install Java",
    )?;
    let vineflower = vineflower(cache, &mut progress)?;
    let _ = fs::remove_dir_all(&out);

    let mut sources = 0;
    let work = out.join(".vineflower");
    for (i, (set, part)) in parts.iter().enumerate() {
        let java_dir = out.join("src").join(set).join("java");
        let res_dir = out.join("src").join(set).join("resources");
        fs::create_dir_all(&java_dir)?;
        fs::create_dir_all(&res_dir)?;
        unpack_resources(part, &res_dir)?;
        let n = class_count(part);
        if n == 0 {
            continue;
        }
        progress(&if parts.len() > 1 {
            format!(
                "Decompiling {set} ({} of {}, {n} classes)",
                i + 1,
                parts.len()
            )
        } else {
            format!("Decompiling {n} classes")
        });
        let input = work.join(format!("{set}.jar"));
        fs::create_dir_all(&work)?;
        fs::write(&input, part)?;
        let dest = work.join(set);
        fs::create_dir_all(&dest)?;
        run_vineflower(&java, &vineflower, &input, &dest)?;
        sources += collect_sources(&dest, &java_dir)?;
    }
    let _ = fs::remove_dir_all(&work);
    if sources == 0 {
        bail!("Vineflower wrote no sources for {}", jar.display());
    }
    fs::write(
        out.join(".decompiled"),
        format!(
            "{}\nVineflower {VINEFLOWER_VERSION}, {sources} source files, {} modules, {libraries} libraries left out\n",
            jar.display(),
            parts.len()
        ),
    )?;
    progress(&format!(
        "Decompiled {sources} source files{}",
        if parts.len() > 1 {
            format!(" from {} modules", parts.len())
        } else {
            String::new()
        }
    ));
    Ok(out)
}

/// A jar's mod id: from `fabric.mod.json`, `quilt.mod.json`, or NeoForge's or
/// Forge's `mods.toml`.
fn mod_id(jar: &[u8]) -> Option<String> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(jar)).ok()?;
    let mut text = |name: &str| -> Option<String> {
        let mut s = String::new();
        archive.by_name(name).ok()?.read_to_string(&mut s).ok()?;
        Some(s)
    };
    if let Some(t) = text("fabric.mod.json") {
        let j: serde_json::Value = serde_json::from_str(&t).ok()?;
        return j["id"].as_str().map(str::to_string);
    }
    if let Some(t) = text("quilt.mod.json") {
        let j: serde_json::Value = serde_json::from_str(&t).ok()?;
        return j["quilt_loader"]["id"].as_str().map(str::to_string);
    }
    for name in ["META-INF/neoforge.mods.toml", "META-INF/mods.toml"] {
        if let Some(t) = text(name) {
            let toml: toml::Table = toml::from_str(&t).ok()?;
            return toml
                .get("mods")?
                .as_array()?
                .first()?
                .get("modId")?
                .as_str()
                .map(str::to_string);
        }
    }
    None
}

/// Whether a nested jar's mod `id` is one of `parent`'s own modules: the same id,
/// or one that starts with it (`c2me-base`, `create_hooks` for `create`).
fn is_module_of(id: &str, parent: &str) -> bool {
    id == parent
        || id
            .strip_prefix(parent)
            .is_some_and(|rest| rest.starts_with(['-', '_', '.']))
}

/// The jars nested in a jar (Fabric's `META-INF/jars/`, NeoForge's
/// `META-INF/jarjar/`): (file name, bytes).
fn nested_jars(jar: &[u8]) -> Result<Vec<(String, Vec<u8>)>> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(jar)).context("reading the jar")?;
    let mut out = Vec::new();
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let name = entry.name()?.to_string();
        let nested = (name.starts_with("META-INF/jars/") || name.starts_with("META-INF/jarjar/"))
            && name.ends_with(".jar");
        if !nested {
            continue;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        let file = name.rsplit('/').next().unwrap_or(&name).to_string();
        out.push((file, data));
    }
    Ok(out)
}

/// The source set a nested module goes in: its id, as a folder name.
fn source_set(id: &str, file: &str) -> String {
    let set: String = id
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || c == '-' || c == '_' {
                c
            } else {
                '_'
            }
        })
        .collect();
    if set.is_empty() || set == "main" {
        file.trim_end_matches(".jar").to_string()
    } else {
        set
    }
}

fn class_count(jar: &[u8]) -> usize {
    zip::ZipArchive::new(std::io::Cursor::new(jar))
        .map(|a| {
            a.file_names()
                .filter(|n| n.as_ref().is_ok_and(|n| n.ends_with(".class")))
                .count()
        })
        .unwrap_or(0)
}

/// Everything in a jar that is no class file and no nested jar, into `res_dir`.
fn unpack_resources(jar: &[u8], res_dir: &Path) -> Result<()> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(jar)).context("reading the jar")?;
    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        if entry.is_dir() {
            continue;
        }
        let Some(rel) = entry.enclosed_name() else {
            continue; // a path that would leave the folder
        };
        if rel.extension().is_some_and(|e| e == "class" || e == "jar") {
            continue;
        }
        let to = res_dir.join(rel);
        if let Some(parent) = to.parent() {
            fs::create_dir_all(parent)?;
        }
        let mut data = Vec::new();
        entry.read_to_end(&mut data)?;
        fs::write(&to, data)?;
    }
    Ok(())
}

fn run_vineflower(java: &Path, vineflower: &Path, input: &Path, dest: &Path) -> Result<()> {
    let result = Command::new(java)
        .arg("-jar")
        .arg(vineflower)
        // Quiet, and keep generic signatures: what the scanner reads. Kotlin
        // classes as Java too: Vineflower's Kotlin plugin writes `.kt` files,
        // which the porter doesn't read (Vein Miner, Fzzy Config).
        .args(["-log=WARN", "-dgs=1", "--kt-enable=0"])
        .arg(input)
        .arg(dest)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .output()
        .with_context(|| format!("running {}", java.display()))?;
    if !result.status.success() {
        bail!(
            "Vineflower could not decompile the jar: {}",
            String::from_utf8_lossy(&result.stderr).trim()
        );
    }
    Ok(())
}

/// Moves the sources Vineflower wrote in `dest` (as files, or as a jar of them)
/// into `java_dir`; how many.
fn collect_sources(dest: &Path, java_dir: &Path) -> Result<usize> {
    let mut sources = 0;
    let mut stack = vec![dest.to_path_buf()];
    while let Some(dir) = stack.pop() {
        for e in fs::read_dir(&dir)?.flatten() {
            let p = e.path();
            if p.is_dir() {
                stack.push(p);
            } else if p.extension().is_some_and(|x| x == "java") {
                let rel = p.strip_prefix(dest).unwrap_or(&p);
                let to = java_dir.join(rel);
                if let Some(parent) = to.parent() {
                    fs::create_dir_all(parent)?;
                }
                fs::rename(&p, &to).or_else(|_| fs::copy(&p, &to).map(|_| ()))?;
                sources += 1;
            } else if p.extension().is_some_and(|x| x == "jar" || x == "zip") {
                let data = fs::read(&p)?;
                let mut inner = zip::ZipArchive::new(std::io::Cursor::new(data))?;
                for i in 0..inner.len() {
                    let mut entry = inner.by_index(i)?;
                    let Some(rel) = entry.enclosed_name() else {
                        continue;
                    };
                    if !rel.extension().is_some_and(|x| x == "java") {
                        continue;
                    }
                    let to = java_dir.join(rel);
                    if let Some(parent) = to.parent() {
                        fs::create_dir_all(parent)?;
                    }
                    let mut text = Vec::new();
                    entry.read_to_end(&mut text)?;
                    fs::write(&to, text)?;
                    sources += 1;
                }
            }
        }
    }
    Ok(sources)
}

/// The jar a decompiled tree came from, if `dir` is one.
pub fn decompiled_from(dir: &Path) -> Option<String> {
    fs::read_to_string(dir.join(".decompiled"))
        .ok()
        .and_then(|t| t.lines().next().map(str::to_string))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hashes_like_maven_central() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        assert_eq!(VINEFLOWER_SHA256.len(), 64);
    }

    #[test]
    fn tells_the_mods_modules_from_libraries() {
        assert!(is_module_of("c2me-base", "c2me"));
        assert!(is_module_of("create_hooks", "create"));
        assert!(is_module_of("c2me", "c2me"));
        assert!(!is_module_of("c2mextra", "c2me"));
        assert!(!is_module_of("com_ibm_async_asyncutil", "c2me"));
        assert_eq!(source_set("c2me-base", "x.jar"), "c2me-base");
        assert_eq!(source_set("main", "lib-1.0.jar"), "lib-1.0");
    }

    #[test]
    fn unpacks_a_jar_without_classes() {
        let dir = std::env::temp_dir().join(format!("porter-decompile-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        let jar = dir.join("empty.jar");
        let mut z = zip::ZipWriter::new(fs::File::create(&jar).unwrap());
        z.start_file("fabric.mod.json", zip::write::SimpleFileOptions::default())
            .unwrap();
        std::io::Write::write_all(&mut z, b"{}").unwrap();
        z.finish().unwrap();
        // No classes (a data-only mod): its resources, without Java or Vineflower.
        let out = decompile(&jar, &dir.join("cache"), |_| {}).unwrap();
        assert!(out.join("src/main/resources/fabric.mod.json").is_file());
        assert!(out.join("src/main/java").is_dir());
        fs::remove_dir_all(&dir).ok();
    }
}
