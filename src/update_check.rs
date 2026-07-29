//! Check whether a newer `onmcu` release has been published.
//!
//! [`spawn`] runs the lookup in the background of every command: cached, and
//! silent about failures. [`check_now`] backs the `update` subcommand: no
//! cache, no opt-outs, and failures are reported.
//!
//! The version comes from the `dist-manifest.json` cargo-dist publishes with
//! every release, served from the release download URL rather than
//! api.github.com -- which avoids the 60 requests/hour-per-IP limit that
//! unauthenticated API calls share, and that busy CI runners do reach.

use std::{
    io::IsTerminal as _,
    path::PathBuf,
    time::{Duration, SystemTime, UNIX_EPOCH},
};

use semver::Version;
use serde::{Deserialize, Serialize};
use thiserror::Error;
use tokio::task::JoinHandle;

/// Version of this build, compared against the latest published release.
const CURRENT_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Release manifest cargo-dist uploads alongside the archives.
const MANIFEST_URL: &str = concat!(
    env!("CARGO_PKG_REPOSITORY"),
    "/releases/latest/download/dist-manifest.json"
);

/// Sent so GitHub can attribute the traffic; also required by api.github.com,
/// should the lookup ever move there.
const USER_AGENT: &str = concat!("onmcu/", env!("CARGO_PKG_VERSION"));

/// How long a fetched version stays valid before the network is consulted again.
const CACHE_TTL: Duration = Duration::from_secs(24 * 60 * 60);

/// How long to wait before retrying after a lookup that produced no version.
///
/// Shorter than [`CACHE_TTL`] because the next attempt may well succeed, but
/// long enough that a blocked network costs one attempt an hour rather than one
/// per command.
const RETRY_TTL: Duration = Duration::from_secs(60 * 60);

/// Cap on the whole HTTP request, so a black-holed connection cannot stall the
/// CLI for the default reqwest timeout. Commands that outlive
/// [`REPORT_BUDGET`] give the lookup this long to finish in the background.
const FETCH_TIMEOUT: Duration = Duration::from_secs(3);

/// Longer cap for [`check_now`]: the user is deliberately waiting on the
/// result, so a slow link should be given a chance rather than reported as a
/// failure.
const FORCED_FETCH_TIMEOUT: Duration = Duration::from_secs(10);

/// How long [`UpdateCheck::report`] waits for a lookup still in flight.
///
/// Spent only on runs that go to the network. It has to cover a cold fetch
/// (~0.5 s: two TLS handshakes, because GitHub redirects to its asset host) or
/// the cache would never be written and every run would start over.
const REPORT_BUDGET: Duration = Duration::from_secs(1);

/// Page for the newest release, so its notes can be read before updating.
const RELEASE_PAGE_URL: &str = concat!(env!("CARGO_PKG_REPOSITORY"), "/releases/latest");

/// The command from the README that installs the newest release.
#[cfg(windows)]
const UPDATE_COMMAND: &str = concat!(
    r#"powershell -ExecutionPolicy Bypass -c "irm "#,
    env!("CARGO_PKG_REPOSITORY"),
    r#"/releases/latest/download/onmcu-installer.ps1 | iex""#
);
#[cfg(not(windows))]
const UPDATE_COMMAND: &str = concat!(
    "curl --proto '=https' --tlsv1.2 -LsSf ",
    env!("CARGO_PKG_REPOSITORY"),
    "/releases/latest/download/onmcu-installer.sh | sh"
);

/// Why a lookup the user asked for could not be completed.
///
/// The background check has no use for these -- it logs and gives up -- so they
/// only ever reach the user through [`check_now`].
#[derive(Debug, Error)]
pub enum UpdateError {
    #[error("Could not reach {MANIFEST_URL}: {0}")]
    Fetch(#[from] reqwest::Error),

    #[error("The release manifest at {MANIFEST_URL} contains no version tag")]
    NoTag,

    #[error("Version {raw:?} is not valid semver: {source}")]
    Version {
        raw: String,
        #[source]
        source: semver::Error,
    },
}

/// Handle to the lookup started by [`spawn`].
#[must_use = "the check prints nothing unless `report` is awaited"]
pub struct UpdateCheck(Option<JoinHandle<Option<Version>>>);

/// Start the check in the background, so the lookup overlaps with the command.
///
/// `command_checks_itself` suppresses it for commands that do their own lookup,
/// keeping every reason to skip the check in one place.
pub fn spawn(command_checks_itself: bool) -> UpdateCheck {
    if command_checks_itself || !is_enabled() {
        return UpdateCheck(None);
    }
    UpdateCheck(Some(tokio::spawn(check())))
}

impl UpdateCheck {
    /// Print the notice, if a newer release exists, after the command is done.
    ///
    /// Gives up after [`REPORT_BUDGET`] rather than delaying the exit; a lookup
    /// that loses the race is simply retried on the next run.
    pub async fn report(self) {
        let Some(handle) = self.0 else { return };

        match tokio::time::timeout(REPORT_BUDGET, handle).await {
            Ok(Ok(Some(latest))) => eprint!("{}", notice(&latest)),
            Ok(Ok(None)) => {}
            Ok(Err(e)) => tracing::debug!("Update check panicked: {e}"),
            Err(_) => tracing::debug!("Update check did not finish within {REPORT_BUDGET:?}"),
        }
    }
}

/// Whether the check should run at all.
fn is_enabled() -> bool {
    // Nothing reads the notice when output is piped or redirected, and CI
    // runners would repeat the lookup for every job.
    if !std::io::stderr().is_terminal() {
        return false;
    }
    !(is_set("ONMCU_NO_UPDATE_CHECK") || is_set("CI"))
}

/// True when `name` is present in the environment and not set to an "off" value.
fn is_set(name: &str) -> bool {
    match std::env::var(name) {
        Ok(value) => !matches!(value.as_str(), "" | "0" | "false"),
        Err(_) => false,
    }
}

/// Look up the newest published release right now, for the `update` subcommand.
///
/// Ignores the cache and every opt-out: an explicit command should answer with
/// what is published today. Prereleases are returned like any other release,
/// unlike the background check, which stays quiet about them.
pub async fn check_now() -> Result<Version, UpdateError> {
    let release = fetch_latest(FORCED_FETCH_TIMEOUT).await?;

    // Saves the next command's background check from repeating this lookup.
    // Prereleases are recorded as "nothing to offer" so it neither surfaces one
    // nor refetches because of one.
    Cache::store((!release.is_prerelease).then(|| release.version.to_string()));

    Ok(release.version)
}

/// Return the latest published version, if it is newer than this build.
///
/// This is the background path, so every failure ends in a debug log and a
/// silent `None`.
async fn check() -> Option<Version> {
    let latest = match Cache::load().filter(Cache::is_fresh) {
        Some(cache) => {
            let cached = cache.latest?;
            Version::parse(&cached)
                .inspect_err(|e| tracing::debug!("Cached version {cached:?} is unusable: {e}"))
                .ok()?
        }
        None => fetch_and_cache().await?,
    };

    // Ordering is semver's, not lexical: 0.10.0 is newer than 0.9.0.
    (latest > current_version()).then_some(latest)
}

/// Fetch the newest release and record the attempt, whatever its outcome.
///
/// Recording it *before* the request matters: a command that finishes first
/// drops this task mid-flight, and an unrecorded attempt would make the next run
/// start over and pay [`REPORT_BUDGET`] again. Failures and prereleases back off
/// for [`RETRY_TTL`] instead of being retried by every command.
async fn fetch_and_cache() -> Option<Version> {
    Cache::store(None);

    let release = fetch_latest(FETCH_TIMEOUT)
        .await
        .inspect_err(|e| tracing::debug!("Update check failed: {e}"))
        .ok()?;

    // Auto-notifying about a prerelease would push users onto a release train
    // they never opted into.
    if release.is_prerelease {
        tracing::debug!("Ignoring prerelease {}", release.version);
        return None;
    }

    Cache::store(Some(release.version.to_string()));
    Some(release.version)
}

/// This build's version.
///
/// Cargo rejects a package whose `version` is not semver, so parsing it cannot
/// fail; `own_version_is_semver` guards that assumption.
pub fn current_version() -> Version {
    Version::parse(CURRENT_VERSION).expect("CARGO_PKG_VERSION is valid semver")
}

/// The newest release, as cargo-dist's manifest describes it.
struct LatestRelease {
    version: Version,
    is_prerelease: bool,
}

/// Fetch the newest published release, giving up after `timeout`.
async fn fetch_latest(timeout: Duration) -> Result<LatestRelease, UpdateError> {
    let client = reqwest::Client::builder()
        .timeout(timeout)
        .user_agent(USER_AGENT)
        .build()?;

    let manifest: DistManifest = client
        .get(MANIFEST_URL)
        .send()
        .await?
        .error_for_status()?
        .json()
        .await?;

    let tag = manifest.announcement_tag.ok_or(UpdateError::NoTag)?;
    let raw = version_from_tag(&tag);

    Ok(LatestRelease {
        version: Version::parse(raw).map_err(|source| UpdateError::Version {
            raw: raw.to_owned(),
            source,
        })?,
        is_prerelease: manifest.announcement_is_prerelease,
    })
}

/// The fields we need out of cargo-dist's release manifest. Unknown keys are
/// ignored, so upstream additions to the schema do not break the check.
#[derive(Deserialize)]
struct DistManifest {
    /// Git tag of the release, conventionally `v<version>`.
    announcement_tag: Option<String>,

    /// `/releases/latest/` already skips prereleases, but cargo-dist's
    /// `force-latest` can publish one as the latest release.
    #[serde(default)]
    announcement_is_prerelease: bool,
}

/// Strip the `v` that cargo-dist puts in front of the version in release tags.
fn version_from_tag(tag: &str) -> &str {
    tag.strip_prefix('v').unwrap_or(tag)
}

/// Outcome of the last lookup, stored so most runs need no network at all.
#[derive(Serialize, Deserialize)]
struct Cache {
    /// When the lookup ran, in seconds since the Unix epoch.
    checked_at: u64,

    /// Latest published version without the tag's `v` prefix, or `None` when
    /// the lookup produced nothing usable -- it failed, was cut short, or the
    /// newest release is a prerelease.
    latest: Option<String>,
}

impl Cache {
    fn load() -> Option<Self> {
        let raw = std::fs::read_to_string(cache_path()?).ok()?;
        serde_json::from_str(&raw).ok()
    }

    /// Best effort: an unwritable cache directory only costs a lookup next run.
    fn store(latest: Option<String>) {
        let Some(path) = cache_path() else { return };
        let cache = Cache {
            checked_at: now_unix(),
            latest,
        };
        let (Ok(json), Some(dir)) = (serde_json::to_string(&cache), path.parent()) else {
            return;
        };
        if let Err(e) = std::fs::create_dir_all(dir).and_then(|()| std::fs::write(&path, json)) {
            tracing::debug!("Could not write update cache to {}: {e}", path.display());
        }
    }

    fn is_fresh(&self) -> bool {
        let ttl = if self.latest.is_some() {
            CACHE_TTL
        } else {
            RETRY_TTL
        };
        // A timestamp in the future means the clock moved; refetch rather than
        // trust an entry that would otherwise stay fresh indefinitely.
        now_unix()
            .checked_sub(self.checked_at)
            .is_some_and(|age| age < ttl.as_secs())
    }
}

/// Per-user cache file, following each platform's convention.
///
/// Deliberately not the config directory that holds the cargo-dist install
/// receipt: this file is derived data and may be deleted at any time.
fn cache_path() -> Option<PathBuf> {
    let dir = if cfg!(windows) {
        PathBuf::from(std::env::var_os("LOCALAPPDATA")?)
    } else if cfg!(target_os = "macos") {
        PathBuf::from(std::env::var_os("HOME")?).join("Library/Caches")
    } else {
        match std::env::var_os("XDG_CACHE_HOME") {
            Some(dir) => PathBuf::from(dir),
            None => PathBuf::from(std::env::var_os("HOME")?).join(".cache"),
        }
    };
    Some(dir.join("onmcu").join("update-check.json"))
}

/// Seconds since the Unix epoch, or 0 if the clock is set before it.
fn now_unix() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |elapsed| elapsed.as_secs())
}

/// The message shown when a newer release exists.
pub fn notice(latest: &Version) -> String {
    format!(
        "\nA new version of onmcu is available: {CURRENT_VERSION} -> {latest}\n\
         Release notes: {RELEASE_PAGE_URL}\n\
         Update with:\n  {UPDATE_COMMAND}\n"
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tag_prefix_is_optional() {
        assert_eq!(version_from_tag("v1.2.3"), "1.2.3");
        assert_eq!(version_from_tag("1.2.3"), "1.2.3");
    }

    #[test]
    fn own_version_is_semver() {
        // The check silently does nothing if this ever stops holding.
        assert!(Version::parse(CURRENT_VERSION).is_ok());
    }

    #[test]
    fn manifest_parsing_covers_the_shapes_dist_produces() {
        // The first case is trimmed from the real dist-manifest.json of v0.1.0.
        for (label, raw, tag, prerelease) in [
            (
                "unknown fields are ignored",
                r#"{
                    "dist_version": "0.32.0",
                    "announcement_tag": "v0.1.0",
                    "announcement_is_prerelease": false,
                    "releases": [{ "app_name": "onmcu", "app_version": "0.1.0" }]
                }"#,
                Some("v0.1.0"),
                false,
            ),
            (
                "a missing prerelease key means a normal release",
                r#"{"announcement_tag": "v0.1.0"}"#,
                Some("v0.1.0"),
                false,
            ),
            (
                "a prerelease is reported as one",
                r#"{"announcement_tag": "v0.2.0-rc.1", "announcement_is_prerelease": true}"#,
                Some("v0.2.0-rc.1"),
                true,
            ),
            // The condition `fetch_latest` turns into `UpdateError::NoTag`.
            (
                "no tag at all",
                r#"{"dist_version": "0.32.0"}"#,
                None,
                false,
            ),
        ] {
            let manifest: DistManifest =
                serde_json::from_str(raw).unwrap_or_else(|e| panic!("{label}: {e}"));
            assert_eq!(manifest.announcement_tag.as_deref(), tag, "{label}");
            assert_eq!(manifest.announcement_is_prerelease, prerelease, "{label}");
        }
    }

    /// A cache entry recorded `age` seconds ago; a negative age is in the future.
    fn cache_aged(age: i64, latest: Option<&str>) -> Cache {
        Cache {
            checked_at: now_unix().saturating_add_signed(-age),
            latest: latest.map(str::to_owned),
        }
    }

    #[test]
    fn cache_freshness_depends_on_age_and_on_whether_a_version_was_found() {
        let found = Some("0.1.0");
        let cache_ttl = CACHE_TTL.as_secs() as i64;
        let retry_ttl = RETRY_TTL.as_secs() as i64;

        for (label, age, latest, fresh) in [
            ("just written", 0, found, true),
            ("inside the cache TTL", cache_ttl - 1, found, true),
            ("past the cache TTL", cache_ttl + 1, found, false),
            // A lookup that found nothing backs off for the shorter RETRY_TTL,
            // so a blocked network costs one attempt an hour, not one per
            // command -- but retries sooner than a successful lookup would.
            ("inside the retry TTL", retry_ttl - 1, None, true),
            ("past the retry TTL", retry_ttl + 1, None, false),
            ("past the retry TTL, but found", retry_ttl + 1, found, true),
            // A clock change must not leave an entry fresh indefinitely.
            ("written in the future", -3600, found, false),
        ] {
            assert_eq!(cache_aged(age, latest).is_fresh(), fresh, "{label}");
        }
    }

    #[test]
    fn cache_file_format_is_stable() {
        // Pins the on-disk shape: a version this CLI wrote must stay readable
        // by other builds, and `latest: null` is how "nothing found" is stored.
        let found = serde_json::to_string(&Cache {
            checked_at: 1_700_000_000,
            latest: Some("0.2.0".to_owned()),
        })
        .expect("should serialize");
        assert_eq!(found, r#"{"checked_at":1700000000,"latest":"0.2.0"}"#);

        let nothing = serde_json::to_string(&Cache {
            checked_at: 1_700_000_000,
            latest: None,
        })
        .expect("should serialize");
        assert_eq!(nothing, r#"{"checked_at":1700000000,"latest":null}"#);
    }

    #[test]
    fn off_values_do_not_enable_a_flag() {
        // Safety: single-threaded test, no other thread reads the environment.
        unsafe {
            for value in ["", "0", "false"] {
                std::env::set_var("ONMCU_TEST_FLAG", value);
                assert!(!is_set("ONMCU_TEST_FLAG"), "{value:?} should not enable");
            }
            for value in ["1", "true", "yes"] {
                std::env::set_var("ONMCU_TEST_FLAG", value);
                assert!(is_set("ONMCU_TEST_FLAG"), "{value:?} should enable");
            }
            std::env::remove_var("ONMCU_TEST_FLAG");
        }
        assert!(!is_set("ONMCU_TEST_FLAG"));
    }

    #[test]
    fn notice_names_both_versions_the_release_page_and_the_install_command() {
        let text = notice(&Version::parse("9.9.9").expect("valid semver"));
        assert!(text.contains(CURRENT_VERSION), "{text}");
        assert!(text.contains("9.9.9"), "{text}");
        // The link comes first, so the notes can be read before running a
        // command that replaces the binary.
        let (Some(link), Some(command)) = (text.find(RELEASE_PAGE_URL), text.find(UPDATE_COMMAND))
        else {
            panic!("both lines should be present: {text}");
        };
        assert!(link < command, "{text}");
    }
}
