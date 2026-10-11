//! `orin update` — check for a newer release and install it in place.
//!
//! Network access lives only here and behind `orin status`' cached update
//! field: `orin query` stays pure local IPC. Every remote call goes through
//! the Windows system tools that ship with the OS (`curl.exe`,
//! `certutil.exe`, `tar.exe`), so the lockfile gains no dependency.
//!
//! Exit codes: 0 = up to date / updated / check unavailable, 2 = the install
//! failed (the previous binary is restored), 3 = `--check` saw an update.

use anyhow::Context;
use anyhow::anyhow;
use anyhow::bail;
use interprocess::local_socket::Stream;
use interprocess::local_socket::prelude::*;
use orin_core::paths::socket_name;
use orin_core::protocol::Request;
use orin_core::protocol::Response;
use orin_core::protocol::read_frame;
use orin_core::protocol::write_frame;
use serde_json::Value;
use std::cmp::Ordering;
use std::io::Read;
use std::path::Path;
use std::path::PathBuf;
use std::process::ExitCode;
use std::process::Stdio;
use std::time::Duration;
use std::time::Instant;
use std::time::SystemTime;
use std::time::UNIX_EPOCH;

/// GitHub's latest-release endpoint. Never reached from the query path.
const RELEASE_API: &str = "https://api.github.com/repos/dnh33/orin/releases/latest";
/// GitHub rejects requests with no user agent; this one is ours.
const USER_AGENT: &str = "orin-update-check";
/// Hard bound on a check request: `status` and `update --check` wait 2s.
const CHECK_TIMEOUT: Duration = Duration::from_secs(2);
/// A download is not a check: two seconds would fail an ordinary zip.
const DOWNLOAD_TIMEOUT: Duration = Duration::from_secs(60);
/// `status` re-checks at most this often (60 anonymous API calls an hour).
const CHECK_TTL_SECS: i64 = 6 * 60 * 60;
/// How long the daemon gets to leave its pipe after `Request::Stop`.
const STOP_WAIT: Duration = Duration::from_secs(2);
/// Read deadline for the daemon's `Stop` acknowledgment.
const STOP_ACK_TIMEOUT: Duration = Duration::from_millis(500);
/// Poll interval while waiting on curl.
const POLL_INTERVAL: Duration = Duration::from_millis(25);
/// Poll interval while waiting on the resident daemon.
const DAEMON_POLL: Duration = Duration::from_millis(100);
/// `update --check` exits with this when a newer release exists.
const UPDATE_AVAILABLE: u8 = 3;
/// One-shot request id, the same shape the IPC client uses.
const REQ_ID: u64 = 1;
/// Length of a SHA-256 digest written as hex.
const SHA256_HEX_LEN: usize = 64;

/// A release worth installing: the version plus the two assets we consume.
struct Release {
    version: String,
    zip_name: String,
    zip_url: String,
    sums_url: String,
}

/// The outcome of a check: the newest release and whether it beats the
/// version this binary reports.
pub(crate) struct Check {
    pub latest: String,
    pub available: bool,
}

/// `orin update`, optionally `--check`: look first, then swap the binary
/// in place while the daemon is stopped.
pub(crate) fn run(check_only: bool) -> anyhow::Result<ExitCode> {
    let release = match fetch_release(CHECK_TIMEOUT) {
        Ok(release) => release,
        Err(err) => {
            println!("update check unavailable: {err:#}");
            return Ok(ExitCode::SUCCESS);
        }
    };
    write_cache(&release.version);
    let latest = release.version.as_str();
    let Some(order) = compare_versions(current_version(), latest) else {
        println!("update check unavailable: release tag {latest} is not a version");
        return Ok(ExitCode::SUCCESS);
    };
    let check = Check {
        latest: latest.to_string(),
        available: order == Ordering::Less,
    };
    let line = report_line(&check);
    println!("{line}");
    if !check.available {
        return Ok(ExitCode::SUCCESS);
    }
    if check_only {
        return Ok(ExitCode::from(UPDATE_AVAILABLE));
    }
    install(&release)?;
    println!("orin updated to {latest}");
    println!("the daemon restarts on your next search");
    Ok(ExitCode::SUCCESS)
}

/// The one line `--check`, `update` and `status` all print for a check.
pub(crate) fn report_line(check: &Check) -> String {
    let current = current_version();
    let latest = check.latest.as_str();
    if check.available {
        format!("update available: {current} -> {latest}, run: orin update")
    } else {
        format!("orin {current} is up to date (latest: {latest})")
    }
}

/// `orin status`' update field: the cached check, at most one request per
/// six hours, and silence (a null field) whenever anything fails.
pub(crate) fn status_check() -> Option<Check> {
    let latest = match fresh_cache() {
        Some(latest) => latest,
        None => {
            let release = fetch_release(CHECK_TIMEOUT).ok()?;
            write_cache(&release.version);
            release.version
        }
    };
    let order = compare_versions(current_version(), &latest);
    let available = matches!(order, Some(Ordering::Less));
    Some(Check { latest, available })
}

/// Naive dotted-version compare, component by component.
///
/// `None` means at least one side is not a dotted number: a check that
/// cannot parse is "no update info", never an error.
pub(crate) fn compare_versions(a: &str, b: &str) -> Option<Ordering> {
    let mut left = a.split('.');
    let mut right = b.split('.');
    loop {
        match (left.next(), right.next()) {
            (None, None) => return Some(Ordering::Equal),
            (x, y) => {
                let order = component(x)?.cmp(&component(y)?);
                if order != Ordering::Equal {
                    return Some(order);
                }
            }
        }
    }
}

/// One dotted component: a missing trailing component reads as zero
/// (`0.1` == `0.1.0`), and anything unparsable is not a version.
fn component(part: Option<&str>) -> Option<u64> {
    match part {
        Some(text) => text.parse().ok(),
        None => Some(0),
    }
}

/// The version this binary reports (`orin --version`).
fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Tags read `v0.1.4`, versions read `0.1.4`.
fn strip_tag(tag: &str) -> &str {
    tag.strip_prefix(['v', 'V']).unwrap_or(tag)
}

/// Read `/releases/latest` and pull out the version and its assets.
fn fetch_release(timeout: Duration) -> anyhow::Result<Release> {
    let dir = scratch_dir("release")?;
    let outcome = read_release(dir.join("release.json"), timeout);
    let _ = std::fs::remove_dir_all(&dir);
    outcome
}

/// The fetch half of [`fetch_release`], with the response file in place.
fn read_release(path: PathBuf, timeout: Duration) -> anyhow::Result<Release> {
    http_get(RELEASE_API, &path, timeout)
        .context("could not read the latest release from GitHub")?;
    let text = std::fs::read_to_string(&path).context("could not read the release response")?;
    let value = serde_json::from_str(&text).context("release response was not JSON")?;
    parse_release(&value)
}

/// Pick the version and the two assets we consume out of the response.
fn parse_release(value: &Value) -> anyhow::Result<Release> {
    let Some(tag) = text(value, "tag_name") else {
        return Err(anyhow!("release response has no tag_name"));
    };
    let version = strip_tag(tag).to_string();
    let mut zip = None;
    let mut sums = None;
    if let Some(assets) = value.get("assets").and_then(Value::as_array) {
        for asset in assets {
            let name = text(asset, "name").unwrap_or_default();
            let url = text(asset, "browser_download_url").unwrap_or_default();
            if name.ends_with("-windows-x86_64.zip") {
                zip = Some((name.to_string(), url.to_string()));
            } else if name == "SHA256SUMS.txt" {
                sums = Some(url.to_string());
            }
        }
    }
    let (zip_name, zip_url) = zip.context("release has no *-windows-x86_64.zip asset")?;
    let sums_url = sums.context("release has no SHA256SUMS.txt asset")?;
    Ok(Release {
        version,
        zip_name,
        zip_url,
        sums_url,
    })
}

/// `object.<key>` as text, the only shape this JSON probe needs.
fn text<'a>(object: &'a Value, key: &str) -> Option<&'a str> {
    object.get(key).and_then(|value| value.as_str())
}

/// Download, verify, swap, refresh: the whole `orin update` install.
fn install(release: &Release) -> anyhow::Result<()> {
    let stage = scratch_dir("update")?;
    let outcome = stage_install(release, &stage);
    let _ = std::fs::remove_dir_all(&stage);
    outcome
}

/// The install itself, with its staging directory handed in so every path
/// can clean up after itself.
fn stage_install(release: &Release, stage: &Path) -> anyhow::Result<()> {
    let zip = stage.join(&release.zip_name);
    let sums = stage.join("SHA256SUMS.txt");
    http_get(&release.zip_url, &zip, DOWNLOAD_TIMEOUT)
        .with_context(|| format!("could not download {}", release.zip_name))?;
    http_get(&release.sums_url, &sums, DOWNLOAD_TIMEOUT)
        .context("could not download SHA256SUMS.txt")?;
    verify_checksum(&zip, &sums, &release.zip_name)
        .with_context(|| format!("checksum failed for {}", release.zip_name))?;

    stop_daemon();

    let dir = install_dir()?;
    let orin = dir.join("orin.exe");
    let old = dir.join("orin.exe.old");
    if old.exists() {
        // A leftover from an update that was interrupted mid-swap.
        let _ = std::fs::remove_file(&old);
    }
    // Windows will rename a running exe but never overwrite it, so the old
    // binary moves out of the way and the fresh one takes its place.
    std::fs::rename(&orin, &old)
        .with_context(|| format!("could not move {} aside", orin.display()))?;

    if let Err(err) = place_binaries(&zip, stage, &dir) {
        // The previous binary is intact as orin.exe.old: put it back.
        let _ = std::fs::remove_file(&orin);
        let restored = std::fs::rename(&old, &orin).is_ok();
        let note = if restored {
            "the previous orin.exe was restored"
        } else {
            "restoring orin.exe from orin.exe.old failed too"
        };
        return Err(err.context(format!("update failed: {note}")));
    }
    remove_old(&old);
    Ok(())
}

/// Unpack the fresh binary into the install folder and refresh `on.exe`.
fn place_binaries(zip: &Path, stage: &Path, dir: &Path) -> anyhow::Result<()> {
    let out = stage.join("extracted");
    std::fs::create_dir_all(&out).context("could not prepare an extraction folder")?;
    extract_zip(zip, &out)?;
    let fresh = find_file(&out, "orin.exe").context("the release zip has no orin.exe")?;
    let target = dir.join("orin.exe");
    std::fs::copy(&fresh, &target)
        .with_context(|| format!("could not write {}", target.display()))?;
    // `on.exe` is a byte-identical copy of orin.exe. Refresh it when it is
    // installed, but never fail an update over the short name.
    let alias = dir.join("on.exe");
    if alias.is_file()
        && let Err(err) = std::fs::copy(&fresh, &alias)
    {
        eprintln!("orin: could not refresh on.exe: {err}");
    }
    Ok(())
}

/// Verify the downloaded archive against the release checksums file
/// before any of it touches the install folder.
fn verify_checksum(zip: &Path, sums: &Path, name: &str) -> anyhow::Result<()> {
    let listed = std::fs::read_to_string(sums).context("could not read SHA256SUMS.txt")?;
    let expected =
        expected_checksum(&listed, name).context("SHA256SUMS.txt does not list this archive")?;
    let actual = sha256_file(zip).map_err(anyhow::Error::msg)?;
    if !actual.eq_ignore_ascii_case(&expected) {
        bail!("sha256 mismatch: expected {expected}, got {actual}");
    }
    Ok(())
}

/// This archive's hash out of a `sha256sum` file (`<hash>  <name>`).
fn expected_checksum(listed: &str, name: &str) -> Option<String> {
    for line in listed.lines() {
        let mut fields = line.split_whitespace();
        let (Some(hash), Some(file)) = (fields.next(), fields.next()) else {
            continue;
        };
        let file = file.trim_start_matches('*');
        let listed_name = Path::new(file).file_name();
        let wanted =
            listed_name.is_some_and(|part| part.to_string_lossy().eq_ignore_ascii_case(name));
        if hash.len() == SHA256_HEX_LEN && wanted {
            return Some(hash.to_ascii_lowercase());
        }
    }
    None
}

/// SHA-256 of a file via `certutil.exe`, lowercased like `sha256sum`.
fn sha256_file(path: &Path) -> Result<String, String> {
    let certutil = system_tool("certutil.exe");
    let output = std::process::Command::new(&certutil)
        .arg("-hashfile")
        .arg(path)
        .arg("SHA256")
        .output()
        .map_err(|err| format!("{} could not run: {err}", certutil.display()))?;
    if !output.status.success() {
        return Err(format!("certutil could not hash {}", path.display()));
    }
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .map(str::trim)
        .find(|line| line.len() == SHA256_HEX_LEN && line.chars().all(|c| c.is_ascii_hexdigit()))
        .map(|line| line.to_ascii_lowercase())
        .ok_or_else(|| "certutil returned no SHA-256 hash".to_string())
}

/// Unpack the archive with the Windows `tar.exe` (bundled bsdtar reads zip).
fn extract_zip(zip: &Path, into: &Path) -> anyhow::Result<()> {
    let tar = system_tool("tar.exe");
    let output = std::process::Command::new(&tar)
        .args(["-xf"])
        .arg(zip)
        .args(["-C"])
        .arg(into)
        .output()
        .with_context(|| format!("{} could not run", tar.display()))?;
    if output.status.success() {
        return Ok(());
    }
    let detail = String::from_utf8_lossy(&output.stderr);
    let detail = detail.trim();
    if detail.is_empty() {
        bail!(
            "could not unpack the release: tar failed with {}",
            output.status
        );
    }
    bail!("could not unpack the release: {detail}");
}

/// Stop the resident daemon so its files can be replaced: ask over the
/// pipe, allow two seconds, then kill what is left. This process is never
/// a candidate, or the update would kill itself.
fn stop_daemon() {
    // Nothing listening means there is no daemon to stop.
    let Some(mut stream) = open_pipe() else {
        return;
    };
    let _ = stream.set_recv_timeout(Some(STOP_ACK_TIMEOUT));
    let _ = write_frame(&mut stream, &Request::Stop { id: REQ_ID });
    let _ack: Option<Response> = read_frame(&mut stream).ok().flatten();
    drop(stream);

    let deadline = Instant::now() + STOP_WAIT;
    loop {
        if open_pipe().is_none() && resident_pids().is_empty() {
            return;
        }
        if Instant::now() >= deadline {
            break;
        }
        std::thread::sleep(DAEMON_POLL);
    }
    kill_resident();
}

/// Connect to the daemon pipe the IPC client uses, when one is listening.
fn open_pipe() -> Option<Stream> {
    let name = socket_name().ok()?;
    Stream::connect(name).ok()
}

/// Force every other `orin.exe` to exit. The daemon is the only resident
/// copy, and `taskkill /IM` would take this updater down with it.
fn kill_resident() {
    for pid in resident_pids() {
        let target = pid.to_string();
        let taskkill = system_tool("taskkill.exe");
        let _ = std::process::Command::new(&taskkill)
            .args(["/PID", target.as_str(), "/F"])
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .status();
    }
}

/// PIDs of resident `orin.exe` processes, minus this one.
fn resident_pids() -> Vec<u32> {
    let tasklist = system_tool("tasklist.exe");
    let Ok(output) = std::process::Command::new(&tasklist)
        .args(["/FI", "IMAGENAME eq orin.exe", "/FO", "CSV", "/NH"])
        .output()
    else {
        return Vec::new();
    };
    let own = std::process::id();
    let text = String::from_utf8_lossy(&output.stdout);
    text.lines()
        .filter_map(|line| {
            let mut fields = line.split(',').map(|field| field.trim().trim_matches('"'));
            let name = fields.next()?;
            let pid = fields.next()?.parse::<u32>().ok()?;
            (name.eq_ignore_ascii_case("orin.exe") && pid != own).then_some(pid)
        })
        .collect()
}

/// The folder the running binary lives in — that is the install folder.
fn install_dir() -> anyhow::Result<PathBuf> {
    let exe = std::env::current_exe().context("could not locate the running orin.exe")?;
    let dir = exe.parent().context("orin.exe has no parent folder")?;
    Ok(dir.to_path_buf())
}

/// Best effort: this process still runs from `orin.exe.old`, so Windows
/// usually refuses the delete until the next run. One retry, then leave it.
fn remove_old(old: &Path) {
    if old.exists() && std::fs::remove_file(old).is_err() {
        std::thread::sleep(Duration::from_secs(1));
        let _ = std::fs::remove_file(old);
    }
}

/// Find a file below `dir`, so an archive that nests its binaries still
/// installs.
fn find_file(dir: &Path, name: &str) -> Option<PathBuf> {
    let entries = std::fs::read_dir(dir).ok()?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_dir()
            && let Some(found) = find_file(&path, name)
        {
            return Some(found);
        }
        let matches = path
            .file_name()
            .is_some_and(|file| file.to_string_lossy().eq_ignore_ascii_case(name));
        if matches {
            return Some(path);
        }
    }
    None
}

/// A Windows system tool, preferring the real System32 copy so a `tar`
/// or `curl` earlier on the user's PATH cannot answer in its place.
fn system_tool(name: &str) -> PathBuf {
    let system32 = std::env::var_os("SystemRoot").map(|root| PathBuf::from(root).join("System32"));
    match system32 {
        Some(dir) if dir.join(name).is_file() => dir.join(name),
        _ => PathBuf::from(name),
    }
}

/// GET `url` into `dest`, killed at `timeout` whatever curl thinks.
///
/// curl's `--max-time` bounds the transfer; the deadline below bounds the
/// process, so a hung resolver can never stall `orin status`.
fn http_get(url: &str, dest: &Path, timeout: Duration) -> anyhow::Result<()> {
    let curl = system_tool("curl.exe");
    let secs = timeout.as_secs().to_string();
    let user_agent = format!("User-Agent: {USER_AGENT}");
    let mut command = std::process::Command::new(&curl);
    command
        .args(["-sS", "-f", "-L", "--max-time", secs.as_str()])
        .args([
            "-H",
            user_agent.as_str(),
            "-H",
            "Accept: application/vnd.github+json",
        ])
        .arg("-o")
        .arg(dest)
        .arg(url)
        .stdout(Stdio::null())
        .stderr(Stdio::piped());
    let mut child = command
        .spawn()
        .with_context(|| format!("{} could not run", curl.display()))?;

    let deadline = Instant::now() + timeout;
    let status = loop {
        match child.try_wait() {
            Ok(Some(status)) => break status,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(POLL_INTERVAL),
            Ok(None) => {
                let _ = child.kill();
                let _ = child.wait();
                bail!("timed out after {}s", timeout.as_secs());
            }
            Err(err) => bail!("waiting for {} failed: {err}", curl.display()),
        }
    };
    if status.success() {
        return Ok(());
    }

    let mut stderr = String::new();
    if let Some(mut pipe) = child.stderr.take() {
        let _ = pipe.read_to_string(&mut stderr);
    }
    let first = stderr.lines().map(str::trim).find(|line| !line.is_empty());
    match first {
        Some(line) => Err(anyhow!("{line}")),
        None => Err(anyhow!("curl failed with {status}")),
    }
}

/// A fresh directory under `%TEMP%`; callers delete it when they are done.
fn scratch_dir(tag: &str) -> anyhow::Result<PathBuf> {
    let nanos = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|elapsed| elapsed.as_nanos())
        .unwrap_or(0);
    let name = format!("orin-{tag}-{}-{nanos}", std::process::id());
    let dir = std::env::temp_dir().join(name);
    std::fs::create_dir_all(&dir).context("could not create a temp directory")?;
    Ok(dir)
}

/// The cached check while it is younger than [`CHECK_TTL_SECS`].
fn fresh_cache() -> Option<String> {
    let text = std::fs::read_to_string(cache_path()).ok()?;
    let value: Value = serde_json::from_str(&text).ok()?;
    let checked_at = value.get("checked_at")?.as_i64()?;
    let latest = value.get("latest")?.as_str()?.to_string();
    let age = unix_now()?.checked_sub(checked_at)?;
    (0..=CHECK_TTL_SECS).contains(&age).then_some(latest)
}

/// Cache the result so `status` waits six hours before asking again.
/// Best effort: an unwritable data dir must not break the check itself.
fn write_cache(latest: &str) {
    let dir = orin_core::paths::data_dir();
    if std::fs::create_dir_all(&dir).is_err() {
        return;
    }
    let Some(checked_at) = unix_now() else {
        return;
    };
    let cached = serde_json::json!({ "checked_at": checked_at, "latest": latest }).to_string();
    let _ = std::fs::write(cache_path(), cached);
}

/// Where the six-hour check result lives.
fn cache_path() -> PathBuf {
    orin_core::paths::data_dir().join("update-check.json")
}

/// Seconds since the Unix epoch; `None` only if the clock predates 1970.
fn unix_now() -> Option<i64> {
    let elapsed = SystemTime::now().duration_since(UNIX_EPOCH).ok()?;
    i64::try_from(elapsed.as_secs()).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The load-bearing comparison: naive dotted semver, and a hard "no
    /// update info" for anything that is not a dotted number.
    #[test]
    fn naive_version_compare() {
        assert_eq!(compare_versions("0.1.4", "0.1.4"), Some(Ordering::Equal));
        assert_eq!(compare_versions("0.1.4", "0.1.3"), Some(Ordering::Greater));
        assert_eq!(compare_versions("0.1.3", "0.1.4"), Some(Ordering::Less));
        assert_eq!(compare_versions("0.1.4-beta", "0.1.3"), None);
        assert_eq!(compare_versions("banana", "0.1.4"), None);
    }
}
