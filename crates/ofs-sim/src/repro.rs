//! The repro bundle (parent spec §7): when a session fails, a folder with everything needed to run it again — the
//! quad, the library it used, the world, the seed, the journal of what steered it, the firmware's state at load and
//! the last seconds of its channels — and the replay that runs the folder and says whether the failure came back.
use std::path::{Path, PathBuf};

use ofs_config::FcKind;
use ofs_core::SimError;
use ofs_log::recorder::{create_numbered, RecordError};
use ofs_log::tail::DEFAULT_WINDOW_S;

use crate::journal::Journal;
use crate::vehicle::{build_seeded, BuildOptions, Vehicle};

/// Version of the folder layout (`bundle.toml`'s `format_version`). 2 exists so a build that predates stabilized
/// sessions refuses their bundles ("this build reads 1") instead of replaying them throttle-only; the layout is
/// otherwise unchanged, so this build still reads the 1-bundles earlier releases wrote.
pub const BUNDLE_FORMAT: u32 = 2;

/// The last bytes of SITL's own log a bundle keeps.
const SITL_LOG_TAIL_BYTES: u64 = 64 * 1024;

/// The files of a firmware directory that make a Betaflight session start the same way: the EEPROM and the diff
/// that produced it.
pub(crate) const FIRMWARE_STATE: [&str; 2] = ["eeprom.bin", "betaflight.diff"];

/// What a session knows about how it was loaded: the inputs of the replay. The configuration files are read when
/// the session loads, not when it fails, so the bundle holds what the session flew even if the files were edited
/// meanwhile.
#[derive(Debug, Clone)]
pub struct BundleSource {
    /// The quad file the session loaded: it names the bundle and the quad inside it.
    pub quad_path: PathBuf,
    pub seed: u64,
    pub open_loop_fc: bool,
    /// With `open_loop_fc`: the built-in stabilizer was on (`Load`'s `stabilize`).
    pub stabilize: bool,
    pub realtime: bool,
    /// Where bundles go: `<logs dir>/<quad stem>-<n>.repro`.
    pub logs_dir: PathBuf,
    /// The Betaflight session's working directory.
    pub firmware_dir: Option<PathBuf>,
    /// `FIRMWARE_STATE` files as they were right after `Load`.
    pub firmware_initial: Vec<(String, Vec<u8>)>,
    /// The quad, its diff, the world and the library, as the session read them: path inside the bundle, bytes.
    pub files: Vec<(PathBuf, Vec<u8>)>,
    /// The quad's path inside the bundle (`quads/<name>.toml`).
    pub quad_rel: String,
    /// The world's path inside the bundle, when the session had one.
    pub world_rel: Option<String>,
}

/// Every regular file under `root`, in sorted order, as (path relative to `root`, bytes).
fn read_tree(root: &Path) -> std::io::Result<Vec<(PathBuf, Vec<u8>)>> {
    fn walk(base: &Path, dir: &Path, out: &mut Vec<(PathBuf, Vec<u8>)>) -> std::io::Result<()> {
        let mut entries: Vec<_> = std::fs::read_dir(dir)?.collect::<Result<_, _>>()?;
        entries.sort_by_key(|e| e.file_name());
        for e in entries {
            let kind = e.file_type()?;
            if kind.is_dir() {
                walk(base, &e.path(), out)?;
            } else if kind.is_file() {
                let rel = e.path().strip_prefix(base).expect("under the root").to_path_buf();
                out.push((rel, std::fs::read(e.path())?));
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    walk(root, root, &mut out)?;
    Ok(out)
}

impl BundleSource {
    /// Reads the configuration files of a session that loaded `cfg` (and `world_path`, if it flew in a world) now.
    /// A Betaflight diff given as an absolute path or with `..` is stored by its file name, and the bundled quad is
    /// rewritten to say so; the files are never copied by path, so nothing can land outside the bundle or onto itself.
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cfg: &ofs_config::QuadConfig,
        world_path: Option<&Path>,
        library_env: Option<&str>,
        seed: u64,
        open_loop_fc: bool,
        realtime: bool,
        logs_dir: PathBuf,
        firmware_dir: Option<PathBuf>,
    ) -> Result<Self, String> {
        let read = |p: &Path, what: &str| std::fs::read(p).map_err(|e| format!("reading the {what} {}: {e}", p.display()));
        let mut files: Vec<(PathBuf, Vec<u8>)> = Vec::new();
        let quad_name = file_name(&cfg.source_path)?;
        let mut quad_bytes = read(&cfg.source_path, "quad")?;
        let diff_rel = cfg.fc.betaflight_diff.as_str();
        let diff_path = cfg.resolve(diff_rel);
        if !diff_rel.is_empty() && diff_path.is_file() {
            let plain = Path::new(diff_rel).components().all(|c| matches!(c, std::path::Component::Normal(_)));
            let stored = if plain { diff_rel.to_string() } else { file_name(&diff_path)? };
            if !plain {
                let text = String::from_utf8(quad_bytes).map_err(|e| format!("the quad file is not UTF-8: {e}"))?;
                let mut table: toml::Table = toml::from_str(&text).map_err(|e| format!("parsing the quad file: {e}"))?;
                if let Some(fc) = table.get_mut("fc").and_then(toml::Value::as_table_mut) {
                    fc.insert("betaflight_diff".into(), toml::Value::String(stored.clone()));
                }
                quad_bytes = toml::to_string(&table).map_err(|e| format!("rewriting the quad file: {e}"))?.into_bytes();
            }
            files.push((Path::new("quads").join(&stored), read(&diff_path, "Betaflight diff")?));
        }
        files.push((Path::new("quads").join(&quad_name), quad_bytes));
        let world_rel = match world_path {
            Some(p) => {
                let name = file_name(p)?;
                files.push((Path::new("worlds").join(&name), read(p, "world")?));
                Some(format!("worlds/{name}"))
            }
            None => None,
        };
        let library = ofs_config::library_root(library_env, cfg.source_dir());
        for (rel, bytes) in read_tree(&library).map_err(|e| format!("reading the library {}: {e}", library.display()))? {
            files.push((Path::new("library").join(rel), bytes));
        }
        Ok(Self {
            quad_path: cfg.source_path.clone(),
            seed,
            open_loop_fc,
            stabilize: false,
            realtime,
            logs_dir,
            firmware_dir,
            firmware_initial: Vec::new(),
            files,
            quad_rel: format!("quads/{quad_name}"),
            world_rel,
        })
    }
}

/// The firmware state files `dir` holds now (none when absent).
pub fn snapshot_firmware(dir: &Path) -> Vec<(String, Vec<u8>)> {
    FIRMWARE_STATE.iter().filter_map(|name| std::fs::read(dir.join(name)).ok().map(|bytes| ((*name).to_string(), bytes))).collect()
}

fn kind_of(e: &SimError) -> &'static str {
    match e {
        SimError::Firmware(_) => "firmware",
        SimError::NonFinite(_) => "non_finite",
        SimError::InvalidArgument(_) => "invalid_argument",
        SimError::Other(_) => "other",
    }
}

fn file_name(p: &Path) -> Result<String, String> {
    p.file_name().map(|n| n.to_string_lossy().into_owned()).ok_or_else(|| format!("{} has no file name", p.display()))
}

fn io<T>(what: &str, r: std::io::Result<T>) -> Result<T, String> {
    r.map_err(|e| format!("{what}: {e}"))
}

/// The FC a session actually flew, as the bundle's two flags: the load request's override, if any, wins over the
/// quad file's kind, and the winner is what the vehicle was built with. `(open_loop_fc, stabilize)`: a true
/// `open_loop_fc` says an open-loop FC flew (plain or with the stabilizer), a true `stabilize` says the stabilizer
/// was on.
pub(crate) fn effective_fc_flags(file_kind: FcKind, requested: Option<FcKind>) -> (bool, bool) {
    match requested.unwrap_or(file_kind) {
        FcKind::OpenLoop => (true, false),
        FcKind::OpenLoopStabilized => (true, true),
        FcKind::Sitl => (false, false),
    }
}

/// Writes the bundle for `vehicle`'s failure `error` and returns its folder. A partial folder is left if a step
/// fails (it carries no `bundle.toml`, which is written last).
pub fn write_bundle(src: &BundleSource, vehicle: &Vehicle, error: &SimError) -> Result<PathBuf, String> {
    use toml::Value;
    let stem = src.quad_path.file_stem().map_or_else(|| "quad".into(), |s| s.to_string_lossy().into_owned());
    let (dir, ()) = create_numbered(&src.logs_dir, &stem, "repro", |p| {
        std::fs::create_dir(p).map_err(|e| RecordError::from_create(p, &e))
    })
    .map_err(|e| format!("creating the repro folder: {e}"))?;

    // The quad, its diff, the library it resolved against and the world: as the session read them at Load.
    for (rel, bytes) in &src.files {
        let target = dir.join(rel);
        if let Some(parent) = target.parent() {
            io(&format!("creating {}", parent.display()), std::fs::create_dir_all(parent))?;
        }
        io(&format!("writing {}", rel.display()), std::fs::write(&target, bytes))?;
    }

    io("writing the journal", vehicle.journal().write(&dir))?;
    let tail_error = match vehicle.snapshot_tail(&dir.join("tail.mcap")) {
        Ok(_) => None,
        Err(e) => Some(e.to_string()),
    };

    if let Some(fw) = &src.firmware_dir {
        io("creating firmware/", std::fs::create_dir_all(dir.join("firmware")))?;
        for (name, bytes) in &src.firmware_initial {
            io("copying the firmware state", std::fs::write(dir.join("firmware").join(name), bytes))?;
        }
        if let Ok(log) = std::fs::read(fw.join("sitl.log")) {
            let keep = SITL_LOG_TAIL_BYTES as usize;
            let tail = &log[log.len().saturating_sub(keep)..];
            io("copying sitl.log", std::fs::write(dir.join("firmware").join("sitl.log"), tail))?;
        }
    }

    let journal = vehicle.journal();
    let exact = !src.realtime && !journal.truncated();
    let mut t = toml::Table::new();
    let mut put = |k: &str, v: Value| {
        t.insert(k.to_string(), v);
    };
    put("format_version", Value::Integer(i64::from(BUNDLE_FORMAT)));
    put("created_by", Value::String(format!("ofs-sim {}", env!("CARGO_PKG_VERSION"))));
    put("reason", Value::String(error.to_string()));
    put("failure_kind", Value::String(kind_of(error).into()));
    put("failed_tick", Value::Integer(vehicle.tick() as i64));
    put("failed_time_s", Value::Float(vehicle.time_s()));
    put("seed", Value::String(src.seed.to_string()));
    put("mode", Value::String(if src.realtime { "realtime" } else { "lockstep" }.into()));
    put("base_hz", Value::Integer(i64::from(vehicle.base_hz())));
    put("open_loop_fc", Value::Boolean(src.open_loop_fc));
    put("stabilize", Value::Boolean(src.stabilize));
    put("quad", Value::String(src.quad_rel.clone()));
    if let Some(w) = &src.world_rel {
        put("world", Value::String(w.clone()));
    }
    put("window_s", Value::Float(DEFAULT_WINDOW_S));
    put("digest", Value::String(format!("{:016x}", vehicle.digest())));
    put("journal_entries", Value::Integer(journal.entries().len() as i64));
    put("journal_truncated", Value::Boolean(journal.truncated()));
    put("exact", Value::Boolean(exact));
    if let Some(e) = tail_error {
        put("tail_error", Value::String(e));
    }
    let text = toml::to_string(&t).map_err(|e| format!("writing bundle.toml: {e}"))?;
    io("writing bundle.toml", std::fs::write(dir.join("bundle.toml"), text))?;
    io("writing README.txt", std::fs::write(dir.join("README.txt"), README))?;
    Ok(dir)
}

const README: &str = "An Open FPV Sim repro bundle: a failed session, with everything needed to run it again.\n\
Replay it:   ofs-sim --replay <this folder>\n\
or, from Python:   import ofs; ofs.repro.replay_bundle(\"<this folder>\")\n\
bundle.toml says what failed and whether the replay is exact. See docs/repro-bundle.md in the repository.\n";

/// What running a bundle again showed.
#[derive(Debug, Clone, PartialEq)]
pub struct ReplayOutcome {
    /// The replay failed with the same error at the same tick and ended on the same digest.
    pub reproduced: bool,
    /// The session was lockstep with an intact journal: a mismatch means the failure is not deterministic.
    pub exact: bool,
    pub reached_tick: u64,
    pub expected_tick: u64,
    /// The replay's own failure, if it had one.
    pub failure: Option<String>,
    pub expected_failure: String,
    pub digest: u64,
    pub expected_digest: u64,
}

/// Runs `f`; a panic becomes `SimError::Other("internal panic: ...")`, the text the server records for one, so a
/// bundle written for a panic replays to a verdict instead of taking the replayer down.
fn guarded<T>(f: impl FnOnce() -> Result<T, SimError>) -> Result<T, SimError> {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(f))
        .unwrap_or_else(|p| Err(SimError::Other(format!("internal panic: {}", crate::session::panic_message(p.as_ref())))))
}

fn text_of<'a>(t: &'a toml::Table, key: &str) -> Result<&'a str, String> {
    t.get(key).and_then(toml::Value::as_str).ok_or_else(|| format!("bundle.toml: missing text key '{key}'"))
}

fn int_of(t: &toml::Table, key: &str) -> Result<i64, String> {
    t.get(key).and_then(toml::Value::as_integer).ok_or_else(|| format!("bundle.toml: missing integer key '{key}'"))
}

fn flag_of(t: &toml::Table, key: &str) -> Result<bool, String> {
    t.get(key).and_then(toml::Value::as_bool).ok_or_else(|| format!("bundle.toml: missing true/false key '{key}'"))
}

/// Runs the bundle at `bundle` again in lockstep, under `data_dir` (firmware state goes there), and compares the
/// result with what `bundle.toml` recorded.
pub fn replay(bundle: &Path, data_dir: &Path) -> Result<ReplayOutcome, String> {
    let text = std::fs::read_to_string(bundle.join("bundle.toml")).map_err(|e| format!("{}: {e}", bundle.join("bundle.toml").display()))?;
    let t: toml::Table = toml::from_str(&text).map_err(|e| format!("bundle.toml: {e}"))?;
    let format = int_of(&t, "format_version")?;
    if format != 1 && format != i64::from(BUNDLE_FORMAT) {
        return Err(format!("bundle.toml: format_version {format} (this build reads 1 and {BUNDLE_FORMAT})"));
    }
    let seed: u64 = text_of(&t, "seed")?.parse().map_err(|_| "bundle.toml: seed is not a number".to_string())?;
    let open_loop = flag_of(&t, "open_loop_fc")?;
    // Bundles written before protocol 12 have no key: they were never stabilized.
    let stabilize = t.get("stabilize").and_then(toml::Value::as_bool).unwrap_or(false);
    let expected_tick = int_of(&t, "failed_tick")? as u64;
    let expected_failure = text_of(&t, "reason")?.to_string();
    let expected_digest =
        u64::from_str_radix(text_of(&t, "digest")?, 16).map_err(|_| "bundle.toml: digest is not hexadecimal".to_string())?;
    let exact = flag_of(&t, "exact")?;

    let quad_path = bundle.join(text_of(&t, "quad")?);
    // The bundle's own library: the replayer's OFS_LIBRARY must not reach it.
    let cfg = ofs_config::load_with_library_env(&quad_path, None).map_err(|e| format!("loading the bundle's quad: {e}"))?;
    let world = match t.get("world").and_then(toml::Value::as_str) {
        Some(w) => ofs_config::world::load(&bundle.join(w)).map_err(|e| format!("loading the bundle's world: {e}"))?,
        None => ofs_config::WorldConfig::open_field(),
    };
    let journal = Journal::read(bundle)?;

    let fc_override = open_loop.then_some(if stabilize { FcKind::OpenLoopStabilized } else { FcKind::OpenLoop });
    let opts = BuildOptions { seed, data_dir: data_dir.to_path_buf(), fc_override, world };
    // The bundle's firmware state goes into the directory of the SITL slot the build takes (not necessarily slot 0).
    let mut vehicle =
        build_seeded(&cfg, &opts, Some(&bundle.join("firmware")), None).map_err(|e| format!("building the bundle's vehicle: {e}"))?;

    let mut failure: Option<SimError> = None;
    'entries: for entry in journal.entries() {
        if entry.tick < vehicle.tick() {
            return Err(format!("journal.txt: an entry at tick {} comes after tick {}", entry.tick, vehicle.tick()));
        }
        let behind = entry.tick - vehicle.tick();
        if let Err(e) = guarded(|| vehicle.step_ticks(behind)) {
            failure = Some(e);
            break 'entries;
        }
        if let Err(e) = guarded(|| vehicle.replay_action(&entry.action)) {
            return Err(format!("replaying the journal entry at tick {}: {e}", entry.tick));
        }
    }
    if failure.is_none() {
        // The failing step starts at `expected_tick`: one more tick is where the original stopped.
        let to_go = expected_tick.saturating_sub(vehicle.tick());
        failure = guarded(|| vehicle.step_ticks(to_go)).and_then(|()| guarded(|| vehicle.step_ticks(1))).err();
    }
    let digest = vehicle.digest();
    let reached_tick = vehicle.tick();
    let failure = failure.map(|e| e.to_string());
    let reproduced = failure.as_deref() == Some(expected_failure.as_str()) && reached_tick == expected_tick && digest == expected_digest;
    Ok(ReplayOutcome { reproduced, exact, reached_tick, expected_tick, failure, expected_failure, digest, expected_digest })
}

/// A bundle source for the shipped quad, open loop, seed 1 (what `session::testing::open_loop_session` builds).
#[cfg(test)]
pub(crate) fn test_source(logs_dir: &Path) -> BundleSource {
    let quad = Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/../../quads/opendrone-5f-freestyle.toml"));
    test_source_for(logs_dir, quad, None, None)
}

/// As `test_source`, for any quad, with an optional world and `OFS_LIBRARY` override (read now, as `Load` reads them).
#[cfg(test)]
pub(crate) fn test_source_for(logs_dir: &Path, quad: &Path, world: Option<&Path>, library_env: Option<&str>) -> BundleSource {
    let quad = std::path::absolute(quad).unwrap();
    let cfg = ofs_config::load_with_library_env(&quad, library_env).unwrap();
    BundleSource::new(&cfg, world, library_env, 1, true, false, logs_dir.to_path_buf(), None).unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::inputs;
    use crate::vehicle::{build, Fault, Sticks};
    use ofs_config::WorldConfig;

    fn repo(rel: &str) -> PathBuf {
        Path::new(env!("CARGO_MANIFEST_DIR")).join("../..").join(rel)
    }

    fn vehicle_from(cfg: &ofs_config::QuadConfig) -> Vehicle {
        let opts = BuildOptions {
            seed: 1,
            data_dir: std::env::temp_dir().join("ofs-repro-unit"),
            fc_override: Some(FcKind::OpenLoop),
            world: WorldConfig::open_field(),
        };
        build(cfg, &opts, None).unwrap()
    }

    /// Full throttle, then a finite fault that overflows the vibration model once the rotors spin: the run stops with
    /// a non-finite value at a tick that depends on when the fault lands.
    fn fail(v: &mut Vehicle) -> SimError {
        v.set_sticks(&Sticks { throttle: 0.8, roll: 0.1, ..Sticks::default() }).unwrap();
        v.step_ticks(400).unwrap();
        v.schedule_fault(Fault::PropDamage { prop: 0, thrust_scale: 1.0, imbalance_kgm: 1e305 }, 0.1).unwrap();
        v.step_ticks(4000).expect_err("the overflowing imbalance stops the run")
    }

    fn files_under(dir: &Path) -> Vec<String> {
        fn walk(base: &Path, dir: &Path, out: &mut Vec<String>) {
            for e in std::fs::read_dir(dir).unwrap() {
                let p = e.unwrap().path();
                if p.is_dir() {
                    walk(base, &p, out);
                } else {
                    out.push(p.strip_prefix(base).unwrap().to_string_lossy().replace('\\', "/"));
                }
            }
        }
        let mut out = Vec::new();
        walk(dir, dir, &mut out);
        out.sort();
        out
    }

    fn bundle_toml(dir: &Path) -> toml::Table {
        toml::from_str(&std::fs::read_to_string(dir.join("bundle.toml")).unwrap()).unwrap()
    }

    #[test]
    fn a_bundle_holds_everything_the_design_lists() {
        let logs = tempfile::tempdir().unwrap();
        let src = test_source(logs.path());
        let mut v = vehicle_from(&ofs_config::load(&src.quad_path).unwrap());
        v.play_inputs(inputs::parse("climb.csv", "t_s,throttle\n0,0.2\n0.01,0.4\n").unwrap()).unwrap();
        let error = fail(&mut v);
        let dir = write_bundle(&src, &v, &error).unwrap();
        assert_eq!(dir, logs.path().join("opendrone-5f-freestyle-1.repro"));
        let files = files_under(&dir);
        for wanted in [
            "README.txt",
            "bundle.toml",
            "inputs-1.csv",
            "journal.txt",
            "quads/opendrone-5f-freestyle.toml",
            "quads/opendrone-5f-freestyle.betaflight.diff",
            "tail.mcap",
            "library/README.md",
            "library/motors/opendrone-openmotor-2207.toml",
            "library/vtx/opendrone-tramp-600.toml",
        ] {
            assert!(files.iter().any(|f| f == wanted), "{wanted} missing from {files:?}");
        }
        assert!(files.iter().all(|f| !f.starts_with("firmware/") && !f.starts_with("worlds/")), "open field, no firmware: {files:?}");
        let library: Vec<_> = files.iter().filter(|f| f.starts_with("library/")).cloned().collect();
        let original = files_under(&repo("library")).into_iter().map(|f| format!("library/{f}")).collect::<Vec<_>>();
        assert_eq!(library, original, "the whole library root");
        let t = bundle_toml(&dir);
        assert_eq!(t["format_version"].as_integer(), Some(i64::from(BUNDLE_FORMAT)));
        assert_eq!(t["failure_kind"].as_str(), Some("non_finite"));
        assert_eq!(t["reason"].as_str(), Some(error.to_string().as_str()));
        assert_eq!(t["failed_tick"].as_integer(), Some(v.tick() as i64));
        assert_eq!(t["seed"].as_str(), Some("1"));
        assert_eq!(t["exact"].as_bool(), Some(true));
        assert_eq!(t["journal_truncated"].as_bool(), Some(false));
        assert_eq!(t["digest"].as_str(), Some(format!("{:016x}", v.digest()).as_str()));
        let bytes = std::fs::read(dir.join("tail.mcap")).unwrap();
        assert!(mcap::Summary::read(&bytes).unwrap().is_some(), "tail.mcap is a finished MCAP file");
    }

    #[test]
    fn a_second_bundle_gets_the_next_number() {
        let logs = tempfile::tempdir().unwrap();
        let src = test_source(logs.path());
        let mut v = vehicle_from(&ofs_config::load(&src.quad_path).unwrap());
        let error = fail(&mut v);
        write_bundle(&src, &v, &error).unwrap();
        let second = write_bundle(&src, &v, &error).unwrap();
        assert_eq!(second.file_name().unwrap(), "opendrone-5f-freestyle-2.repro");
    }

    #[test]
    fn a_world_is_copied_and_named() {
        let logs = tempfile::tempdir().unwrap();
        let quad = repo("quads/opendrone-5f-freestyle.toml");
        let src = test_source_for(logs.path(), &quad, Some(&repo("worlds/flat.toml")), None);
        let mut v = vehicle_from(&ofs_config::load(&src.quad_path).unwrap());
        let error = fail(&mut v);
        let dir = write_bundle(&src, &v, &error).unwrap();
        assert!(dir.join("worlds/flat.toml").is_file());
        assert_eq!(bundle_toml(&dir)["world"].as_str(), Some("worlds/flat.toml"));
    }

    fn copy_dir(from: &Path, to: &Path) {
        for (rel, bytes) in read_tree(from).unwrap() {
            let target = to.join(rel);
            std::fs::create_dir_all(target.parent().unwrap()).unwrap();
            std::fs::write(target, bytes).unwrap();
        }
    }

    /// A copy of `quads/` (the shipped quad and its diff) and `library/` under `root`, the library's motor part
    /// changed to 2100 KV.
    fn private_tree(root: &Path) -> PathBuf {
        std::fs::create_dir_all(root.join("quads")).unwrap();
        for name in ["opendrone-5f-freestyle.toml", "opendrone-5f-freestyle.betaflight.diff"] {
            std::fs::copy(repo("quads").join(name), root.join("quads").join(name)).unwrap();
        }
        copy_dir(&repo("library"), &root.join("library"));
        let motor = root.join("library/motors/opendrone-openmotor-2207.toml");
        let text = std::fs::read_to_string(&motor).unwrap().replace("kv_rpm_per_v = 1950.0", "kv_rpm_per_v = 2100.0");
        std::fs::write(&motor, text).unwrap();
        root.join("quads/opendrone-5f-freestyle.toml")
    }

    #[test]
    fn the_bundle_carries_the_library_the_session_used_and_replays_with_it() {
        let work = tempfile::tempdir().unwrap();
        let logs = tempfile::tempdir().unwrap();
        let quad = private_tree(&work.path().join("my tree")); // a path with a space
        // As OFS_LIBRARY would say: a library that is not the one beside the quads.
        let elsewhere = work.path().join("elsewhere");
        copy_dir(&work.path().join("my tree/library"), &elsewhere);
        std::fs::remove_dir_all(work.path().join("my tree/library")).unwrap();
        let library_env = elsewhere.to_string_lossy().into_owned();
        let src = test_source_for(logs.path(), &quad, None, Some(&library_env));
        let cfg = ofs_config::load_with_library_env(&quad, Some(&library_env)).unwrap();
        assert_eq!(cfg.motors[0].kv_rpm_per_v, 2100.0, "the private library is in effect");
        let mut v = vehicle_from(&cfg);
        let error = fail(&mut v);
        let dir = write_bundle(&src, &v, &error).unwrap();
        let copied = std::fs::read_to_string(dir.join("library/motors/opendrone-openmotor-2207.toml")).unwrap();
        assert!(copied.contains("2100.0"), "the library the session used, not the shipped one");
        let outcome = replay(&dir, &work.path().join("data")).unwrap();
        assert!(outcome.reproduced, "{outcome:?}");
        assert!(outcome.exact);
    }

    #[test]
    fn a_missing_inputs_file_is_named_by_the_replay() {
        let logs = tempfile::tempdir().unwrap();
        let src = test_source(logs.path());
        let mut v = vehicle_from(&ofs_config::load(&src.quad_path).unwrap());
        v.play_inputs(inputs::parse("climb.csv", "t_s,throttle\n0,0.2\n0.01,0.4\n").unwrap()).unwrap();
        let error = fail(&mut v);
        let dir = write_bundle(&src, &v, &error).unwrap();
        std::fs::remove_file(dir.join("inputs-1.csv")).unwrap();
        let message = replay(&dir, &logs.path().join("data")).unwrap_err();
        assert!(message.contains("inputs-1.csv"), "{message}");
    }

    #[test]
    fn a_truncated_journal_is_written_and_replays_inexactly() {
        let logs = tempfile::tempdir().unwrap();
        let src = test_source(logs.path());
        let mut v = vehicle_from(&ofs_config::load(&src.quad_path).unwrap());
        // Alternating sticks: every call is a new entry.
        for i in 0..=crate::journal::MAX_ENTRIES {
            v.set_sticks(&Sticks { throttle: if i % 2 == 0 { 0.2 } else { 0.3 }, ..Sticks::default() }).unwrap();
        }
        assert!(v.journal().truncated());
        let error = fail(&mut v);
        let dir = write_bundle(&src, &v, &error).unwrap();
        let t = bundle_toml(&dir);
        assert_eq!((t["journal_truncated"].as_bool(), t["exact"].as_bool()), (Some(true), Some(false)));
        assert!(std::fs::read_to_string(dir.join("journal.txt")).unwrap().lines().any(|l| l == "# truncated"));
        let outcome = replay(&dir, &logs.path().join("data")).unwrap();
        assert!(!outcome.exact, "{outcome:?}");
    }

    #[test]
    fn the_bundle_holds_the_files_as_the_session_loaded_them_not_as_they_are_when_it_fails() {
        let work = tempfile::tempdir().unwrap();
        let logs = tempfile::tempdir().unwrap();
        let quad = private_tree(work.path()); // the motor part says 2100 KV
        let src = test_source_for(logs.path(), &quad, None, None);
        let mut v = vehicle_from(&ofs_config::load(&quad).unwrap());
        // The user tunes the files while the server is up.
        let motor = work.path().join("library/motors/opendrone-openmotor-2207.toml");
        std::fs::write(&motor, std::fs::read_to_string(&motor).unwrap().replace("2100.0", "1500.0")).unwrap();
        std::fs::write(&quad, "this is no longer a quad file").unwrap();
        let error = fail(&mut v);
        let dir = write_bundle(&src, &v, &error).unwrap();
        let copied = std::fs::read_to_string(dir.join("library/motors/opendrone-openmotor-2207.toml")).unwrap();
        assert!(copied.contains("2100.0") && !copied.contains("1500.0"), "the part as loaded");
        assert!(std::fs::read_to_string(dir.join("quads/opendrone-5f-freestyle.toml")).unwrap().contains("schema_version"));
        let outcome = replay(&dir, &work.path().join("data")).unwrap();
        assert!(outcome.reproduced && outcome.exact, "{outcome:?}");
    }

    #[test]
    fn a_diff_given_by_an_absolute_path_or_with_dots_is_stored_by_name() {
        let diff_name = "opendrone-5f-freestyle.betaflight.diff";
        for form in ["absolute", "dots"] {
            let work = tempfile::tempdir().unwrap();
            let logs = tempfile::tempdir().unwrap();
            let quad = private_tree(work.path());
            let elsewhere = work.path().join("elsewhere");
            std::fs::create_dir_all(&elsewhere).unwrap();
            let original = std::fs::read(work.path().join("quads").join(diff_name)).unwrap();
            std::fs::rename(work.path().join("quads").join(diff_name), elsewhere.join(diff_name)).unwrap();
            let written = match form {
                "absolute" => format!("'{}'", elsewhere.join(diff_name).display()),
                _ => format!("\"../elsewhere/{diff_name}\""),
            };
            let before = std::fs::read_to_string(&quad).unwrap();
            let after = before.replace(&format!("betaflight_diff = \"{diff_name}\""), &format!("betaflight_diff = {written}"));
            assert_ne!(before, after, "{form}: the quad names its diff");
            std::fs::write(&quad, after).unwrap();

            let src = test_source_for(logs.path(), &quad, None, None);
            let mut v = vehicle_from(&ofs_config::load(&quad).unwrap());
            let error = fail(&mut v);
            let dir = write_bundle(&src, &v, &error).unwrap_or_else(|e| panic!("{form}: {e}"));
            assert_eq!(std::fs::read(dir.join("quads").join(diff_name)).unwrap(), original, "{form}: the diff, by name");
            assert_eq!(std::fs::read(elsewhere.join(diff_name)).unwrap(), original, "{form}: the user's file is untouched");
            let bundled = ofs_config::load_with_library_env(&dir.join("quads/opendrone-5f-freestyle.toml"), None).unwrap();
            assert_eq!(bundled.fc.betaflight_diff, diff_name, "{form}");
            let outcome = replay(&dir, &work.path().join("data")).unwrap();
            assert!(outcome.reproduced, "{form}: {outcome:?}");
        }
    }

    #[test]
    fn a_panic_during_a_replay_is_a_failure_with_the_servers_text_not_a_crash() {
        let r: Result<(), SimError> = guarded(|| panic!("boom"));
        assert_eq!(r, Err(SimError::Other("internal panic: boom".into())));
        assert_eq!(guarded(|| Ok(3)), Ok(3));
        assert_eq!(guarded::<()>(|| Err(SimError::Firmware("x".into()))), Err(SimError::Firmware("x".into())));
    }

    #[test]
    fn an_unusable_bundle_is_an_error_not_a_verdict() {
        let dir = tempfile::tempdir().unwrap();
        assert!(replay(dir.path(), dir.path()).unwrap_err().contains("bundle.toml"));
        std::fs::write(dir.path().join("bundle.toml"), "format_version = 99\n").unwrap();
        assert!(replay(dir.path(), dir.path()).unwrap_err().contains("format_version 99"));
    }

    #[test]
    fn the_bundle_flags_say_what_actually_flew() {
        use ofs_config::FcKind;
        assert_eq!(effective_fc_flags(FcKind::OpenLoopStabilized, None), (true, true), "the file's kind, no override");
        assert_eq!(effective_fc_flags(FcKind::OpenLoopStabilized, Some(FcKind::OpenLoopStabilized)), (true, true));
        assert_eq!(effective_fc_flags(FcKind::OpenLoop, Some(FcKind::OpenLoop)), (true, false));
        assert_eq!(effective_fc_flags(FcKind::Sitl, None), (false, false));
        assert_eq!(effective_fc_flags(FcKind::Sitl, Some(FcKind::OpenLoop)), (true, false), "a SITL quad overridden to open loop");
    }

    /// A bundle written by a build that still said `format_version = 1` (0.2.0 and earlier, before stabilized
    /// sessions) must replay on this build: only layout version 2 exists beyond it, and its meaning is unchanged.
    #[test]
    fn a_format_1_bundle_still_replays() {
        let dir = tempfile::tempdir().unwrap();
        std::fs::write(
            dir.path().join("bundle.toml"),
            "format_version = 1\nseed = \"1\"\nopen_loop_fc = false\nstabilize = false\nfailed_tick = 5\nreason = \"x\"\ndigest = \"0\"\nexact = true\nquad = \"missing.toml\"\n",
        )
        .unwrap();
        let err = replay(dir.path(), dir.path()).unwrap_err();
        assert!(!err.contains("format_version"), "the version check passed: {err}");
        assert!(err.contains("missing.toml"), "{err}");
    }
}
