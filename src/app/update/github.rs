//! Asking GitHub which release is newest: the two addresses, the version
//! and release read out of what comes back, and the made-up release the
//! demo switch offers.

use bevy::prelude::*;
use std::time::Duration;

/// The two places GitHub answers "which release is newest": the REST
/// API, which says so in JSON with the release notes attached, and the
/// releases page, which redirects to the newest one and says nothing else.
///
/// The API is asked first and is the one that gives the page its notes.
/// It is also rationed: sixty answers an hour to everyone behind one
/// public address, and a hall of players launching the game behind one
/// router is exactly the busy afternoon that runs it dry. When it is dry
/// the page is asked instead, which is not rationed that way, so the
/// offer still stands, only without the notes.
///
/// Both are built from the manifest's repository so a fork asks about
/// itself.
pub struct Where {
    pub api: String,
    pub page: String,
}

pub(super) fn latest_release_urls() -> Option<Where> {
    let repo = env!("CARGO_PKG_REPOSITORY").strip_prefix("https://github.com/")?;
    Some(Where {
        api: format!("https://api.github.com/repos/{repo}/releases/latest"),
        page: format!("https://github.com/{repo}/releases/latest"),
    })
}

/// How long the check may take, connect to last byte, before the thread
/// gives up. Short: on a slow line the answer is worth less than the
/// bother of a page appearing after the player has settled in, and
/// nothing on the menu waits on it either way.
const TIMEOUT: Duration = Duration::from_secs(3);

/// A release version. Semver underneath, so a pre-release sorts below
/// the release it precedes: a `0.2.0-rc.1` build is told when `0.2.0` is
/// out, and an `rc` tagged latest is offered as the `rc` it is.
#[derive(Clone, PartialEq, Eq, PartialOrd, Ord, Debug)]
pub struct Version(semver::Version);

impl Version {
    /// `1.2.3`, `v1.2.3`, `v1.2.3-rc.1`. Anything else is not a version.
    pub fn parse(tag: &str) -> Option<Version> {
        let tag = tag.trim();
        let tag = tag.strip_prefix(['v', 'V']).unwrap_or(tag);
        semver::Version::parse(tag).ok().map(Version)
    }

    /// The version this binary was built as.
    pub fn current() -> Version {
        // The manifest's version is a valid semver or cargo would not
        // have built this; a test says so as well.
        Version::parse(env!("CARGO_PKG_VERSION"))
            .unwrap_or_else(|| Version(semver::Version::new(0, 0, 0)))
    }
}

impl std::fmt::Display for Version {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "v{}", self.0)
    }
}

/// What GitHub said the newest release is.
#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Release {
    pub version: Version,
    /// The tag as GitHub spells it (`v1.2.0`), for the log and the page.
    pub tag: String,
    /// The release page, for the browser.
    pub url: String,
    /// The release notes, as written: markdown, usually.
    pub notes: String,
}

/// What a release page's address must look like before it is handed to
/// a browser (or, on Windows, to `cmd`): on GitHub, over https, and made
/// of nothing a shell has opinions about. No percent, in particular:
/// `cmd` expands `%NAME%`, and a release page of ours has never needed
/// an escape. The reply is GitHub's own over TLS, so this is belt to
/// those braces, but the address is the one thing here that leaves the
/// process.
fn is_release_page(url: &str) -> bool {
    url.starts_with("https://github.com/")
        && url.chars().all(|c| {
            c.is_ascii_alphanumeric() || matches!(c, '-' | '.' | '_' | '~' | '/' | ':' | '+')
        })
}

/// Read a release out of the GitHub API's `releases/latest` reply. `None`
/// for anything that is not one, a tag that is not a version included.
pub fn parse_release(json: &str) -> Option<Release> {
    let value: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag = value.get("tag_name")?.as_str()?.trim();
    let version = Version::parse(tag)?;
    let url = value.get("html_url")?.as_str()?.trim();
    if !is_release_page(url) {
        return None;
    }
    let notes = value
        .get("body")
        .and_then(serde_json::Value::as_str)
        .unwrap_or("")
        .to_string();
    Some(Release {
        version,
        tag: tag.to_string(),
        url: url.to_string(),
        notes,
    })
}

/// Ask GitHub. Blocking; runs on the check thread. `None` for any kind of
/// no: offline, too slow, no release yet, or an answer that is not a
/// release. Each is logged at a level nobody is shown, and none is the
/// player's problem.
pub fn fetch_latest(urls: &Where) -> Option<Release> {
    let agent = ureq::Agent::config_builder()
        .timeout_global(Some(TIMEOUT))
        .user_agent(concat!("pinch-points/", env!("CARGO_PKG_VERSION")))
        // The page's answer *is* its redirect; following it would fetch a
        // release page's worth of HTML to learn what the header said.
        .max_redirects(0)
        .build()
        .new_agent();
    let release = match ask_api(&agent, &urls.api) {
        Ok(release) => release,
        // 403 is how the API says its hourly ration is spent (429 is what
        // it is documented to say). Not fatal: the page knows the tag.
        Err(ureq::Error::StatusCode(403 | 429)) => {
            info!("update check: the API is rationed out; asking the releases page");
            ask_page(&agent, &urls.page)
        }
        Err(e) => {
            info!("update check: {e}");
            None
        }
    };
    match &release {
        Some(release) => info!("update check: newest release is {}", release.tag),
        None => info!("update check: no release to offer"),
    }
    release
}

/// The API's answer, notes and all. `Ok(None)` for a reply that is not a
/// release; the error for a reply that is not a 2xx.
fn ask_api(agent: &ureq::Agent, url: &str) -> Result<Option<Release>, ureq::Error> {
    let body = agent
        .get(url)
        .header("Accept", "application/vnd.github+json")
        .call()?
        .body_mut()
        .read_to_string()?;
    Ok(parse_release(&body))
}

/// The page's answer: the tag in the redirect, and no notes.
fn ask_page(agent: &ureq::Agent, url: &str) -> Option<Release> {
    let response = match agent.get(url).call() {
        Ok(response) => response,
        Err(e) => {
            info!("update check: {e}");
            return None;
        }
    };
    if !response.status().is_redirection() {
        info!("update check: the releases page did not redirect");
        return None;
    }
    let location = response.headers().get("location")?.to_str().ok()?;
    release_from_redirect(location)
}

/// A release read off where the releases page sends the browser:
/// `.../releases/tag/v1.2.0`. Notes are not to be had this way.
pub fn release_from_redirect(location: &str) -> Option<Release> {
    let location = location.trim();
    let (_, tag) = location.rsplit_once("/releases/tag/")?;
    if !is_release_page(location) || tag.is_empty() || tag.contains('/') {
        return None;
    }
    let version = Version::parse(tag)?;
    Some(Release {
        version,
        tag: tag.to_string(),
        url: location.to_string(),
        notes: String::new(),
    })
}

/// The made-up release `PINCH_UPDATE_DEMO` offers.
pub(super) fn demo_release() -> Release {
    let mut version = Version::current();
    version.0.minor += 1;
    version.0.patch = 0;
    version.0.pre = semver::Prerelease::EMPTY;
    Release {
        tag: version.to_string(),
        url: format!("{}/releases", env!("CARGO_PKG_REPOSITORY")),
        notes: "## What's new\n\n\
                - A **new tide event**: the undertow, which drags every loose crab one tile seaward.\n\
                - Two more Beach Day challenges.\n\
                - The lobby names the beach that went off the air instead of just dropping it.\n\n\
                ## Fixes\n\n\
                - Pairs mode no longer overflows the settings row in German.\n\
                - See [the changelog](https://example.invalid/changelog) for the rest.\n"
            .to_string(),
        version,
    }
}

#[cfg(test)]
mod tests {
    use super::super::UpdateCheck;
    use super::super::notes::notes_lines;
    use super::*;

    #[test]
    fn versions_parse_and_order() {
        let v = |s| Version::parse(s);
        assert_eq!(v("v1.2.3"), Some(Version(semver::Version::new(1, 2, 3))));
        assert_eq!(v("1.2.3"), v("v1.2.3"));
        assert_eq!(v(" V1.2.3 "), v("v1.2.3"));
        // A pre-release is a version, below the release it precedes.
        assert!(v("v1.2.3-rc.1").is_some());
        assert!(v("v1.2.3-rc.1") < v("v1.2.3"));
        assert!(v("v1.2.3-rc.1") > v("v1.2.2"));
        assert!(v("v1.2.3+build.7").is_some());
        assert_eq!(v("v1.2"), None);
        assert_eq!(v("v1.2.3.4"), None);
        assert_eq!(v("latest"), None);
        assert_eq!(v(""), None);
        assert_eq!(v("v1.x.3"), None);
        assert!(v("v1.10.0") > v("v1.9.9"));
        assert!(v("v2.0.0") > v("v1.99.99"));
        assert!(v("v0.1.1") > v("v0.1.0"));
        assert_eq!(v("v0.1.0").unwrap().to_string(), "v0.1.0");
        assert_eq!(v("0.2.0-rc.1").unwrap().to_string(), "v0.2.0-rc.1");
    }

    /// The manifest's version parses, or the check compares against
    /// 0.0.0 and offers every release ever made.
    #[test]
    fn this_build_has_a_version() {
        assert_eq!(
            Some(Version::current()),
            Version::parse(env!("CARGO_PKG_VERSION"))
        );
        assert!(Version::current() > Version::parse("0.0.0").unwrap());
    }

    /// The addresses come from the manifest, so a fork asks about itself
    /// rather than about this repository.
    #[test]
    fn the_addresses_name_the_repository() {
        let urls = latest_release_urls().expect("a GitHub repository");
        assert_eq!(
            urls.api,
            "https://api.github.com/repos/agourlay/pinch-points/releases/latest"
        );
        assert_eq!(
            urls.page,
            "https://github.com/agourlay/pinch-points/releases/latest"
        );
    }

    /// The rationed-out fallback: the redirect names the tag, and only
    /// a redirect to a release tag counts.
    #[test]
    fn a_redirect_to_a_tag_is_a_release_without_notes() {
        let release =
            release_from_redirect("https://github.com/agourlay/pinch-points/releases/tag/v0.3.1")
                .expect("a release");
        assert_eq!(release.tag, "v0.3.1");
        assert_eq!(release.version, Version::parse("0.3.1").unwrap());
        assert_eq!(release.notes, "");
        assert!(release.url.ends_with("/releases/tag/v0.3.1"));
        assert_eq!(
            release_from_redirect("https://github.com/agourlay/pinch-points/releases"),
            None
        );
        assert_eq!(
            release_from_redirect("https://github.com/agourlay/pinch-points/releases/tag/"),
            None
        );
        assert_eq!(
            release_from_redirect("https://github.com/x/y/releases/tag/nightly"),
            None
        );
        assert_eq!(
            release_from_redirect("https://evil.example/x/y/releases/tag/v1.0.0"),
            None
        );
        assert_eq!(
            release_from_redirect("http://github.com/x/y/releases/tag/v1.0.0"),
            None
        );
    }

    /// The shape of GitHub's `releases/latest` reply, cut down to the
    /// fields read. Everything else in it is ignored, and a body that is
    /// missing or null is empty notes rather than no release.
    #[test]
    fn a_github_reply_is_read_as_a_release() {
        let json = r#"{
            "url": "https://api.github.com/repos/agourlay/pinch-points/releases/1",
            "html_url": "https://github.com/agourlay/pinch-points/releases/tag/v0.2.0",
            "tag_name": "v0.2.0",
            "name": "Pinch Points 0.2.0",
            "draft": false,
            "prerelease": false,
            "body": "New tide\r\n\r\n- Undertow\r\n",
            "assets": []
        }"#;
        let release = parse_release(json).expect("a release");
        assert_eq!(release.tag, "v0.2.0");
        assert_eq!(release.version, Version::parse("0.2.0").unwrap());
        assert_eq!(
            release.url,
            "https://github.com/agourlay/pinch-points/releases/tag/v0.2.0"
        );
        assert_eq!(release.notes, "New tide\r\n\r\n- Undertow\r\n");

        let no_body = r#"{"tag_name": "v0.2.0", "html_url": "https://github.com/x/y/releases/tag/v0.2.0", "body": null}"#;
        assert_eq!(parse_release(no_body).unwrap().notes, "");
    }

    /// What is not a release: GitHub's 404 body, a tag that is not a
    /// version, a page that is not https, and nonsense.
    #[test]
    fn what_is_not_a_release() {
        assert_eq!(
            parse_release(r#"{"message": "Not Found", "status": "404"}"#),
            None
        );
        assert_eq!(
            parse_release(r#"{"tag_name": "nightly", "html_url": "https://github.com/x/y"}"#),
            None
        );
        assert_eq!(
            parse_release(r#"{"tag_name": "v1.0.0", "html_url": "http://github.com/x/y"}"#),
            None
        );
        // Off GitHub, or carrying something a shell would read: not ours.
        assert_eq!(
            parse_release(r#"{"tag_name": "v1.0.0", "html_url": "https://example.com/x"}"#),
            None
        );
        assert_eq!(
            parse_release(
                r#"{"tag_name": "v1.0.0", "html_url": "https://github.com/x/y/releases/tag/v1.0.0 & calc"}"#
            ),
            None
        );
        assert_eq!(
            parse_release(r#"{"tag_name": "v1.0.0", "html_url": "javascript:alert(1)"}"#),
            None
        );
        assert_eq!(parse_release("<html>rate limited</html>"), None);
        assert_eq!(parse_release(""), None);
    }

    /// The demo release is a real one as far as the page is concerned:
    /// newer than this build, and its notes exercise the flattening.
    #[test]
    fn the_demo_release_is_offered() {
        let demo = demo_release();
        assert!(UpdateCheck::worth_offering(&Version::current(), &demo));
        assert!(demo.url.starts_with("https://github.com/"));
        assert!(notes_lines(&demo.notes, 12).len() > 4);
    }
}
