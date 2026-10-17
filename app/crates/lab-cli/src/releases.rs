//! Building the runtime of a Carafe release or commit once and keeping it, so a plan can run main against a release
//! and a bisection can try the commits between two versions.
//!
//! Everything happens in Linux containers with the lab's build limits, so Git for Windows never touches line
//! endings. Each revision is checked out in one clone of the Carafe checkout, `<work>/releases/carafe`; its
//! submodules come from the checkout's own submodules and Autorun's vendored sources are copied, so nothing is
//! downloaded; a revision whose submodule commits the checkout does not have stops with that message instead. The
//! first time, the checkout's build folder is copied too, with every file dated long ago except the files the revision
//! changes, so only those are built again; later revisions build on what the one before left.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use carafe_pack::archive::write_archive;
use carafe_pack::verify::verify_runtime;
pub use lab_run::releases::tag;
use lab_run::releases::{PREFIX, kept};

const BUILD_CPUS: &str = "2";
const BUILD_JOBS: &str = "2";
const BUILD_MEMORY: &str = "4g";
const CHECKOUT_IMAGE: &str = "carafe-runtime";
const RELEASE_IMAGE: &str = "carafe-lab-release";
const SOURCE_DIR: &str = "carafe";

const SEED: &str = r#"set -eu
cd /releases
if [ ! -d "$DIR/.git" ]; then
  rm -rf "$DIR"
  git clone -q --no-checkout /main "$DIR"
fi
cd "$DIR"
git checkout -q --detach "$REV"
local_url() { printf 'submodule.%s.url=/main/%s' "$1" "$1"; }
for path in external/autorun external/nx-hbloader; do
  if ! git -c protocol.file.allow=always -c "$(local_url "$path")" submodule update --init -q "$path"; then
    echo "$REV needs a commit of $path that the Carafe checkout does not have" >&2
    exit 3
  fi
done
if ! git -C external/autorun -c protocol.file.allow=always \
    -c submodule.horizon-dlls.url=/main/external/autorun/horizon-dlls submodule update --init -q horizon-dlls; then
  echo "$REV needs a commit of horizon-dlls that the Carafe checkout does not have" >&2
  exit 3
fi
vendor=external/autorun/horizon-wine/vendor
mkdir -p "$vendor"
for name in box64 ffmpeg; do
  [ -d "$vendor/$name" ] || cp -a "/main/$vendor/$name" "$vendor/$name"
done
if [ ! -d runtime/build ] && [ -d /main/runtime/build ]; then
  cp -a /main/runtime/build runtime/build
  find . -path ./.git -prune -o -path ./runtime/build -prune -o -exec touch -h -d 2000-01-01 {} +
  git diff --name-only "$(git -C /main rev-parse HEAD)" HEAD | while read -r file; do
    [ -e "$file" ] && touch "$file"
  done
fi
"#;

/// Returns the runtime archive `spec` names: `release:<tag>` or `release:latest` for a kept release runtime, any other
/// text as a file.
///
/// # Errors
///
/// A message when the release is unknown or its runtime is not built yet, or the file is missing.
pub fn runtime(spec: &str, work: &Path, carafe: &Path) -> Result<(PathBuf, String), String> {
    if let Some(name) = spec.strip_prefix(PREFIX) {
        let tag = tag(carafe, name)?;
        let file = kept(work, &tag);
        if !file.is_file() {
            return Err(format!(
                "the runtime of {tag} is not built yet: run carafe-lab release {tag}"
            ));
        }
        return Ok((file, tag));
    }
    let file = PathBuf::from(spec);
    if !file.is_file() {
        return Err(format!("{spec}: no such runtime archive"));
    }
    let label = file.file_name().map_or_else(
        || spec.to_owned(),
        |name| name.to_string_lossy().into_owned(),
    );
    Ok((file, label))
}

/// Builds the runtime of the release `tag` from the Carafe checkout `carafe` and keeps it, unless it is kept already.
///
/// # Returns
///
/// The kept archive.
///
/// # Errors
///
/// A message when the release cannot be cloned or needs submodule commits the checkout lacks, the container build
/// fails or the runtime cannot be packed.
pub fn build(
    carafe: &Path,
    work: &Path,
    tag: &str,
    say: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    build_revision(carafe, work, tag, &kept(work, tag), say)
}

/// Builds the runtime of the Carafe commit `commit` and keeps it, unless it is kept already.
///
/// # Returns
///
/// The kept archive.
///
/// # Errors
///
/// As [`build`].
pub fn build_commit(
    carafe: &Path,
    work: &Path,
    commit: &str,
    say: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let out = commit_kept(work, commit);
    build_revision(carafe, work, commit, &out, say)
}

/// Returns where the runtime of the Carafe commit `commit` is kept.
#[must_use]
pub fn commit_kept(work: &Path, commit: &str) -> PathBuf {
    work.join("runtimes").join(format!("commit-{commit}.bin"))
}

fn build_revision(
    carafe: &Path,
    work: &Path,
    revision: &str,
    out: &Path,
    say: &mut dyn FnMut(&str),
) -> Result<PathBuf, String> {
    let tag = revision;
    let out = out.to_path_buf();
    if out.is_file() {
        say(&format!("{tag}: kept at {}", out.display()));
        return Ok(out);
    }
    let releases = work.join("releases");
    fs::create_dir_all(&releases).map_err(|error| error.to_string())?;
    let source = releases.join(SOURCE_DIR);
    say(&format!(
        "{tag}: cloning with the checkout's submodules into {}",
        source.display()
    ));
    run(limited(Command::new("docker").arg("run"))
        .arg("-v")
        .arg(format!("{}:/main:ro", carafe.display()))
        .arg("-v")
        .arg(format!("{}:/releases", releases.display()))
        .args([
            "-e",
            &format!("REV={revision}"),
            "-e",
            &format!("DIR={SOURCE_DIR}"),
            CHECKOUT_IMAGE,
            "sh",
            "-c",
            SEED,
        ]))?;
    let dockerfile = |root: &Path| fs::read(root.join("docker").join("Dockerfile")).ok();
    if dockerfile(&source).is_none() || dockerfile(&source) != dockerfile(carafe) {
        return Err(format!(
            "{tag} builds in another image than the Carafe checkout; building that image downloads its tools, so it \
             is left to you: docker build -t {RELEASE_IMAGE}:{tag} {}",
            source.join("docker").display()
        ));
    }
    let image = format!("{RELEASE_IMAGE}:{tag}");
    run(Command::new("docker").args(["tag", CHECKOUT_IMAGE, &image]))?;
    say(&format!(
        "{tag}: building the runtime ({BUILD_CPUS} CPUs, {BUILD_JOBS} jobs, {BUILD_MEMORY})"
    ));
    run(limited(Command::new("docker").arg("run"))
        .arg("-v")
        .arg(format!("{}:/work", source.display()))
        .args(["-w", "/work", "-e", &format!("JOBS={BUILD_JOBS}"), &image])
        .args(["make", "-C", "runtime"]))?;
    say(&format!("{tag}: packing the runtime"));
    pack(
        &source
            .join("app")
            .join("crates")
            .join("carafe-desktop")
            .join("resources")
            .join("runtime"),
        &out,
    )?;
    say(&format!("{tag}: kept at {}", out.display()));
    Ok(out)
}

/// Packs the runtime folder `runtime` into the archive `out`, through a partial file so a cut-short pack leaves none.
///
/// # Errors
///
/// A message when the runtime's DLLs do not match its manifest or the archive cannot be written.
pub fn pack(runtime: &Path, out: &Path) -> Result<(), String> {
    verify_runtime(&runtime.join("romfs"), &mut |_| {}).map_err(|error| error.to_string())?;
    if let Some(dir) = out.parent() {
        fs::create_dir_all(dir).map_err(|error| error.to_string())?;
    }
    let partial = out.with_extension("part");
    write_archive(runtime, &partial, &mut |_| {}).map_err(|error| error.to_string())?;
    fs::rename(&partial, out).map_err(|error| error.to_string())
}

fn limited(command: &mut Command) -> &mut Command {
    command
        .args(["--rm", "--cpus", BUILD_CPUS, "--memory", BUILD_MEMORY])
        .args([
            "-e",
            "GIT_CONFIG_COUNT=1",
            "-e",
            "GIT_CONFIG_KEY_0=safe.directory",
        ])
        .args(["-e", "GIT_CONFIG_VALUE_0=*"])
}

fn run(command: &mut Command) -> Result<(), String> {
    let status = command.status().map_err(|error| error.to_string())?;
    if status.success() {
        Ok(())
    } else {
        Err(format!("{command:?} failed with {status}"))
    }
}
