//! The update check against a server of our own: the two ways GitHub
//! answers and the one way the network does not, driven over plain HTTP
//! on the loopback so nothing here reaches the internet.

use pinch_points::app::update::github::{Version, Where, fetch_latest};
use std::io::{Read, Write};
use std::net::TcpListener;

/// A one-shot HTTP/1.1 server on a port of the system's choosing. Each
/// request is answered by path from `reply`, which returns the status
/// line's reason and the extra headers and body to send. It serves, on a
/// thread of its own, for as long as the test process lasts.
fn serve(reply: impl Fn(&str) -> (u16, Vec<(String, String)>, String) + Send + 'static) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("a loopback port");
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut raw = Vec::new();
            let mut buf = [0u8; 1024];
            while let Ok(n) = stream.read(&mut buf) {
                raw.extend_from_slice(&buf[..n]);
                if n == 0 || raw.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let head = String::from_utf8_lossy(&raw);
            let path = head.split_whitespace().nth(1).unwrap_or("/").to_string();
            let (status, headers, body) = reply(&path);
            let mut out = format!("HTTP/1.1 {status} Whatever\r\nConnection: close\r\n");
            for (name, value) in headers {
                out.push_str(&format!("{name}: {value}\r\n"));
            }
            out.push_str(&format!("Content-Length: {}\r\n\r\n{body}", body.len()));
            let _ = stream.write_all(out.as_bytes());
            let _ = stream.flush();
        }
    });
    port
}

fn addresses(port: u16) -> Where {
    Where {
        api: format!("http://127.0.0.1:{port}/api"),
        page: format!("http://127.0.0.1:{port}/page"),
    }
}

/// The API rationed out (403) hands over to the releases page, whose
/// redirect names the tag: a release with a version and no notes. The
/// redirect is not followed, so the target need not exist.
#[test]
fn a_rationed_api_falls_back_to_the_page_redirect() {
    let port = serve(|path| match path {
        "/api" => (
            403,
            vec![],
            r#"{"message":"API rate limit exceeded"}"#.into(),
        ),
        "/page" => (
            302,
            vec![(
                "Location".into(),
                "https://github.com/agourlay/pinch-points/releases/tag/v99.0.0".into(),
            )],
            String::new(),
        ),
        _ => (404, vec![], String::new()),
    });
    let release = fetch_latest(&addresses(port)).expect("a release off the redirect");
    assert_eq!(release.tag, "v99.0.0");
    assert_eq!(release.version, Version::parse("v99.0.0").unwrap());
    assert_eq!(
        release.url,
        "https://github.com/agourlay/pinch-points/releases/tag/v99.0.0"
    );
    assert_eq!(release.notes, "", "the page knows the tag and nothing else");
}

/// The API answering: the release as GitHub spells it, notes attached,
/// and the page never asked.
#[test]
fn the_api_answer_is_a_release_with_notes() {
    let port = serve(|path| match path {
        "/api" => (
            200,
            vec![("Content-Type".into(), "application/json".into())],
            r#"{"tag_name": "v98.1.0",
                "html_url": "https://github.com/agourlay/pinch-points/releases/tag/v98.1.0",
                "body": "Big tide\r\n\r\n- Undertow\r\n"}"#
                .into(),
        ),
        _ => panic!("the page was asked though the API answered: {path}"),
    });
    let release = fetch_latest(&addresses(port)).expect("a release off the API");
    assert_eq!(release.tag, "v98.1.0");
    assert_eq!(release.version, Version::parse("v98.1.0").unwrap());
    assert_eq!(release.notes, "Big tide\r\n\r\n- Undertow\r\n");
}

/// Nobody listening is a refused connection, and that is `None` at once:
/// the check thread must not hang the length of the timeout, let alone
/// past it, on a machine with no network.
#[test]
fn a_port_nobody_listens_on_is_no_release() {
    // Bind and drop: the port was free a moment ago and is free again.
    let port = TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let started = std::time::Instant::now();
    assert_eq!(fetch_latest(&addresses(port)), None);
    assert!(
        started.elapsed() < std::time::Duration::from_secs(10),
        "a refused connection took {:?} to give up",
        started.elapsed()
    );
}
