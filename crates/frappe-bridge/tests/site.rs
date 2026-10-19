//! The transport against a site that is not real —
//! [32](../../../docs/specs/32-testing.md).
//!
//! A bench's site is a database, and pointing tests at one would make the test
//! suite depend on a running service and, worse, on its contents. This is a
//! socket that speaks just enough `/api/v2` to answer the three calls the
//! bridge makes, and records what it was asked.

use std::sync::{Arc, Mutex};

use frappe_bridge::{Client, Credentials, sync};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpListener;

/// What the fake site was asked for, so a test can assert on the request as
/// well as on the answer.
#[derive(Debug, Default)]
struct Log {
    requests: Vec<String>,
}

/// Serve `pages` in order, then empty pages forever.
async fn serve(pages: Vec<String>, log: Arc<Mutex<Log>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr");

    tokio::spawn(async move {
        let mut served = 0usize;
        loop {
            let Ok((stream, _)) = listener.accept().await else {
                return;
            };
            let log = Arc::clone(&log);
            let body = pages.get(served).cloned().unwrap_or_else(|| {
                // Frappe answers an over-the-end page with an empty list, and
                // that is what stops the bridge's paging loop.
                r#"{"data":[]}"#.to_string()
            });
            served += 1;

            tokio::spawn(async move {
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                }
                log.lock().expect("lock").requests.push(request);

                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = reader.into_inner().write_all(response.as_bytes()).await;
            });
        }
    });

    format!("http://{address}")
}

fn credentials(site: &str) -> Credentials {
    // Not through `resolve`: that reads the environment, and a test that sets
    // process-wide variables races every other test in the binary.
    Credentials::new(site, "key:secret", frappe_bridge::Source::Environment)
}

#[tokio::test]
async fn a_sync_reads_the_three_record_types_with_a_token() {
    let log = Arc::new(Mutex::new(Log::default()));
    let site = serve(
        vec![
            r#"{"data":[{"name":"Item-custom_note","dt":"Item","fieldname":"custom_note","fieldtype":"Small Text"}]}"#.to_string(),
            r#"{"data":[{"name":"Item-main-search_fields","doc_type":"Item","field_name":"","property":"search_fields","value":"item_name"}]}"#.to_string(),
            r#"{"data":[{"name":"Loyalty Tier","module":"Custom","custom":1}]}"#.to_string(),
        ],
        Arc::clone(&log),
    )
    .await;

    let client = Client::new(credentials(&site)).expect("client");
    let read = sync::read(&client).await.expect("sync");

    assert_eq!(read.custom_fields.len(), 1);
    assert_eq!(read.property_setters.len(), 1);
    assert_eq!(read.custom_doctypes.len(), 1);
    assert_eq!(read.len(), 3);

    let requests = log.lock().expect("lock").requests.clone();
    assert_eq!(requests.len(), 3, "one list call each");

    // Read-only, and authenticated, and asking for the right things. Header
    // names are case-insensitive and the transport lowercases them.
    for request in &requests {
        assert!(request.starts_with("GET "), "not a GET: {request}");
        assert!(
            request
                .to_lowercase()
                .contains("authorization: token key:secret"),
            "no token: {request}"
        );
    }
    assert!(requests[0].contains("Custom%20Field"), "{}", requests[0]);
    assert!(requests[1].contains("Property%20Setter"), "{}", requests[1]);
    // The custom-DocType call is filtered rather than reading every DocType.
    assert!(requests[2].contains("custom"), "{}", requests[2]);
}

/// A site that pages the way `/api/v2/document/<doctype>` does: `start` and
/// `limit`, `limit` defaulting to 20, and `has_next_page` in the answer. A
/// fake that served pages whatever was asked could not see a client sending
/// v1's parameter names, which v2 ignores.
async fn serve_rows(rows: Vec<String>, log: Arc<Mutex<Log>>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr");
    let rows = Arc::new(rows);

    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            let log = Arc::clone(&log);
            let rows = Arc::clone(&rows);
            tokio::spawn(async move {
                let mut reader = BufReader::new(stream);
                let mut request = String::new();
                loop {
                    let mut line = String::new();
                    if reader.read_line(&mut line).await.unwrap_or(0) == 0 {
                        return;
                    }
                    if line == "\r\n" {
                        break;
                    }
                    request.push_str(&line);
                }
                let query = request.lines().next().unwrap_or("").to_string();
                log.lock().expect("lock").requests.push(request);

                let parameter = |name: &str| {
                    query
                        .split(['?', '&', ' '])
                        .find_map(|pair| pair.strip_prefix(&format!("{name}=")))
                        .and_then(|value| value.parse::<usize>().ok())
                };
                let start = parameter("start").unwrap_or(0).min(rows.len());
                let limit = parameter("limit").unwrap_or(20);
                let end = (start + limit).min(rows.len());
                let body = format!(
                    r#"{{"data":[{}],"has_next_page":{}}}"#,
                    rows[start..end].join(","),
                    end < rows.len()
                );
                let response = format!(
                    "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = reader.into_inner().write_all(response.as_bytes()).await;
            });
        }
    });

    format!("http://{address}")
}

#[tokio::test]
async fn paging_reads_every_row_in_pages_of_500() {
    let rows: Vec<String> = (0..1234)
        .map(|i| format!(r#"{{"name":"f{i}","dt":"Item","fieldname":"f{i}"}}"#))
        .collect();
    let log = Arc::new(Mutex::new(Log::default()));
    let site = serve_rows(rows, Arc::clone(&log)).await;

    let client = Client::new(credentials(&site)).expect("client");
    let read: Vec<serde_json::Value> = client
        .list("Custom Field", &["name"], None)
        .await
        .expect("list");

    // Every row: a client that asks with names v2 does not read gets 20.
    assert_eq!(read.len(), 1234);
    let requests = log.lock().expect("lock").requests.clone();
    assert_eq!(requests.len(), 3, "500 + 500 + 234");
    assert!(requests[0].contains("start=0&limit=500"), "{}", requests[0]);
    assert!(requests[2].contains("start=1000&limit=500"), "{}", requests[2]);
    // Unstable paging silently drops rows, so the order is pinned.
    assert!(requests[0].contains("order_by=name"), "{}", requests[0]);
}

#[tokio::test]
async fn a_refusal_is_an_error_and_not_an_empty_sync() {
    // An empty answer and a refused one mean opposite things: one is a site
    // with no customizations, the other is a picture the server does not have.
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
    let address = listener.local_addr().expect("addr");
    tokio::spawn(async move {
        while let Ok((stream, _)) = listener.accept().await {
            tokio::spawn(async move {
                let mut reader = BufReader::new(stream);
                let mut line = String::new();
                while reader.read_line(&mut line).await.unwrap_or(0) > 0 {
                    if line == "\r\n" {
                        break;
                    }
                    line.clear();
                }
                let body = r#"{"exception":"frappe.PermissionError"}"#;
                let response = format!(
                    "HTTP/1.1 403 Forbidden\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                    body.len()
                );
                let _ = reader.into_inner().write_all(response.as_bytes()).await;
            });
        }
    });

    let client = Client::new(credentials(&format!("http://{address}"))).expect("client");
    let error = sync::read(&client).await.expect_err("a refusal");
    assert!(
        matches!(error, frappe_bridge::Error::Rejected { status: 403, .. }),
        "{error:?}"
    );
}
