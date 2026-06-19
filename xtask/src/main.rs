//! Repository automation tasks (`cargo xtask <command>`).

use std::path::{Path, PathBuf};
use std::process::Command;

const DEFAULT_REGISTRY_REPO: &str = "https://github.com/ethereum/clear-signing-erc7730-registry";

fn main() {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let result = match args.first().map(String::as_str) {
        Some("update-registry-snapshot") => {
            update_registry_snapshot(args.get(1).map(String::as_str))
        }
        _ => {
            eprintln!("usage: cargo xtask update-registry-snapshot [registry-repo-url]");
            std::process::exit(2);
        }
    };
    if let Err(e) = result {
        eprintln!("error: {e}");
        std::process::exit(1);
    }
}

/// Refresh the bundled ERC-7730 registry snapshot embedded via the
/// `bundled-registry` feature (include_dir).
///
/// Vendors:
/// - `index.calldata.json`, `index.eip712.json` (split v3 indexes)
/// - `registry/` and `ercs/` (JSON files only, preserving repo-root-relative layout)
/// - `SNAPSHOT.rev` (upstream commit hash)
fn update_registry_snapshot(repo_url: Option<&str>) -> Result<(), String> {
    let repo_url = repo_url.unwrap_or(DEFAULT_REGISTRY_REPO);
    let root_dir = workspace_root()?;
    let snapshot_dir = root_dir.join("crates/clear-signing/src/assets/registry-snapshot");

    let clone_dir = TempDir::new()?;
    let clone_path = clone_dir.path();

    eprintln!("Cloning {repo_url} ...");
    run(Command::new("git")
        .args(["clone", "--depth", "1", repo_url])
        .arg(clone_path))?;
    let rev = run_capture(
        Command::new("git")
            .args(["-C"])
            .arg(clone_path)
            .args(["rev-parse", "HEAD"]),
    )?;
    let rev = rev.trim();

    for index in ["index.calldata.json", "index.eip712.json"] {
        if !clone_path.join(index).is_file() {
            return Err(format!(
                "upstream repo is missing {index} (split v3 index required)"
            ));
        }
    }

    if snapshot_dir.exists() {
        std::fs::remove_dir_all(&snapshot_dir)
            .map_err(|e| format!("remove {}: {e}", snapshot_dir.display()))?;
    }
    std::fs::create_dir_all(&snapshot_dir)
        .map_err(|e| format!("create {}: {e}", snapshot_dir.display()))?;

    let mut count = 0usize;
    for index in ["index.calldata.json", "index.eip712.json"] {
        std::fs::copy(clone_path.join(index), snapshot_dir.join(index))
            .map_err(|e| format!("copy {index}: {e}"))?;
        count += 1;
    }
    for dir in ["registry", "ercs"] {
        count += copy_json_tree(&clone_path.join(dir), &snapshot_dir.join(dir))?;
    }

    let pruned = prune_stale_index_entries(&snapshot_dir)?;

    std::fs::write(snapshot_dir.join("SNAPSHOT.rev"), format!("{rev}\n"))
        .map_err(|e| format!("write SNAPSHOT.rev: {e}"))?;

    eprintln!(
        "Snapshot updated: {count} JSON files at upstream revision {rev} \
         ({pruned} stale index entries pruned)"
    );
    Ok(())
}

/// Drop index entries that reference descriptor files absent from the
/// upstream repo (stale upstream index data), so the embedded snapshot is
/// self-consistent: every indexed path is guaranteed to exist in the tree.
/// Returns the number of pruned entries.
fn prune_stale_index_entries(snapshot_dir: &Path) -> Result<usize, String> {
    let mut pruned = 0usize;

    // index.calldata.json: key → "relative/path.json"
    let calldata_path = snapshot_dir.join("index.calldata.json");
    let mut calldata = read_json(&calldata_path)?;
    let map = calldata
        .as_object_mut()
        .ok_or("index.calldata.json: expected top-level object")?;
    map.retain(|key, path| {
        let keep = path
            .as_str()
            .is_some_and(|p| snapshot_dir.join(p).is_file());
        if !keep {
            pruned += 1;
            eprintln!("pruning stale calldata index entry: {key} -> {path}");
        }
        keep
    });
    write_json(&calldata_path, &calldata)?;

    // index.eip712.json: key → { primaryType → [ { path, ... } ] }
    let eip712_path = snapshot_dir.join("index.eip712.json");
    let mut eip712 = read_json(&eip712_path)?;
    let map = eip712
        .as_object_mut()
        .ok_or("index.eip712.json: expected top-level object")?;
    map.retain(|key, buckets| {
        let Some(buckets) = buckets.as_object_mut() else {
            return true;
        };
        buckets.retain(|primary_type, entries| {
            let Some(entries) = entries.as_array_mut() else {
                return true;
            };
            entries.retain(|entry| {
                let keep = entry
                    .get("path")
                    .and_then(|p| p.as_str())
                    .is_some_and(|p| snapshot_dir.join(p).is_file());
                if !keep {
                    pruned += 1;
                    eprintln!("pruning stale eip712 index entry: {key} {primary_type} -> {entry}");
                }
                keep
            });
            !entries.is_empty()
        });
        !buckets.is_empty()
    });
    write_json(&eip712_path, &eip712)?;

    Ok(pruned)
}

fn read_json(path: &Path) -> Result<serde_json::Value, String> {
    let body =
        std::fs::read_to_string(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    serde_json::from_str(&body).map_err(|e| format!("parse {}: {e}", path.display()))
}

fn write_json(path: &Path, value: &serde_json::Value) -> Result<(), String> {
    let body = serde_json::to_string_pretty(value)
        .map_err(|e| format!("serialize {}: {e}", path.display()))?;
    std::fs::write(path, body + "\n").map_err(|e| format!("write {}: {e}", path.display()))
}

/// Recursively copy `*.json` files from `src` to `dst`, preserving layout.
/// Directories that contain no JSON files are not created. Returns file count.
fn copy_json_tree(src: &Path, dst: &Path) -> Result<usize, String> {
    let entries = std::fs::read_dir(src).map_err(|e| format!("read {}: {e}", src.display()))?;
    let mut count = 0usize;
    for entry in entries {
        let entry = entry.map_err(|e| format!("read entry in {}: {e}", src.display()))?;
        let path = entry.path();
        let file_type = entry
            .file_type()
            .map_err(|e| format!("file type {}: {e}", path.display()))?;
        if file_type.is_dir() {
            count += copy_json_tree(&path, &dst.join(entry.file_name()))?;
        } else if file_type.is_file() && path.extension().is_some_and(|ext| ext == "json") {
            std::fs::create_dir_all(dst).map_err(|e| format!("create {}: {e}", dst.display()))?;
            std::fs::copy(&path, dst.join(entry.file_name()))
                .map_err(|e| format!("copy {}: {e}", path.display()))?;
            count += 1;
        }
    }
    Ok(count)
}

fn workspace_root() -> Result<PathBuf, String> {
    // xtask lives at <root>/xtask, so the parent of CARGO_MANIFEST_DIR is the root.
    let manifest_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest_dir
        .parent()
        .map(Path::to_path_buf)
        .ok_or_else(|| "cannot determine workspace root".to_string())
}

fn run(cmd: &mut Command) -> Result<(), String> {
    let status = cmd.status().map_err(|e| format!("spawn {cmd:?}: {e}"))?;
    if !status.success() {
        return Err(format!("command failed ({status}): {cmd:?}"));
    }
    Ok(())
}

fn run_capture(cmd: &mut Command) -> Result<String, String> {
    let output = cmd.output().map_err(|e| format!("spawn {cmd:?}: {e}"))?;
    if !output.status.success() {
        return Err(format!("command failed ({}): {cmd:?}", output.status));
    }
    String::from_utf8(output.stdout).map_err(|e| format!("non-UTF-8 output from {cmd:?}: {e}"))
}

struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Result<Self, String> {
        let dir = std::env::temp_dir().join(format!(
            "registry-snapshot-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|e| e.to_string())?
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).map_err(|e| format!("create temp dir: {e}"))?;
        Ok(Self(dir))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
