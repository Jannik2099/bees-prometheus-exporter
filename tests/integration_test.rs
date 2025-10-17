use assert_cmd::cargo::cargo_bin;
use hyper::{Client, StatusCode, Uri, body::to_bytes, client::HttpConnector};
use std::net::{Ipv6Addr, TcpListener};
use std::process::{Command, Stdio};
use std::time::Duration;

type HyperClient = Client<HttpConnector, hyper::Body>;

async fn fetch_metrics(client: &HyperClient, uri: &Uri) -> Result<String, String> {
    let mut last_error: Option<String> = None;

    for _ in 0..50 {
        match client.get(uri.clone()).await {
            Ok(response) => {
                if response.status() != StatusCode::OK {
                    last_error = Some(format!("unexpected status: {}", response.status()));
                } else {
                    match to_bytes(response.into_body()).await {
                        Ok(bytes) => match String::from_utf8(bytes.to_vec()) {
                            Ok(body) => return Ok(body),
                            Err(err) => {
                                return Err(format!(
                                    "failed to decode response body as UTF-8: {}",
                                    err
                                ));
                            }
                        },
                        Err(err) => {
                            last_error = Some(format!("failed to read response body: {}", err));
                        }
                    }
                }
            }
            Err(err) => {
                last_error = Some(format!("request failed: {}", err));
            }
        }

        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    Err(last_error.unwrap_or_else(|| "timed out waiting for exporter metrics response".to_string()))
}

struct ChildGuard(std::process::Child);

impl Drop for ChildGuard {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

#[tokio::test(flavor = "current_thread")]
#[cfg_attr(miri, ignore)]
async fn exporter_serve_metrics_from_tests_directory() {
    let binary = cargo_bin!("bees-prometheus-exporter");
    let tests_dir = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut connector = HttpConnector::new();
    connector.set_nodelay(true);
    let client: HyperClient = Client::builder().build(connector);

    let mut attempt_errors: Vec<String> = Vec::new();
    let mut body: Option<String> = None;

    for attempt in 0..5 {
        let port = {
            let listener = TcpListener::bind((Ipv6Addr::LOCALHOST, 0))
                .expect("Failed to bind to an ephemeral IPv6 port");
            listener
                .local_addr()
                .expect("Failed to read listener address")
                .port()
        };

        let uri: Uri = format!("http://[::1]:{}/metrics", port)
            .parse()
            .expect("Failed to parse metrics URI");

        let _child = ChildGuard(
            Command::new(binary)
                .arg("--bees-work-dir")
                .arg(&tests_dir)
                .arg("--address")
                .arg("::1")
                .arg("--port")
                .arg(port.to_string())
                .stdout(Stdio::null())
                .stderr(Stdio::null())
                .spawn()
                .expect("Failed to spawn bees-prometheus-exporter"),
        );

        match fetch_metrics(&client, &uri).await {
            Ok(metrics) => {
                body = Some(metrics);
                break;
            }
            Err(error) => {
                attempt_errors.push(format!(
                    "attempt {} using port {}: {}",
                    attempt + 1,
                    port,
                    error
                ));
            }
        }
    }

    let body = match body {
        Some(body) => body,
        None if attempt_errors.is_empty() => {
            panic!("Failed to retrieve metrics after retries with no diagnostic errors");
        }
        None => {
            panic!(
                "Failed to retrieve metrics after retries: {}",
                attempt_errors.join("; ")
            );
        }
    };

    assert!(
        body.contains("bees_crawl_done"),
        "Metrics body should contain bees_crawl_done counter.\n{}",
        body
    );
    assert!(
        body.contains("uuid=\"0cadef6c-c480-41f2-95b7-511609815820\""),
        "Metrics body should include uuid labels from the tests directory.\n{}",
        body
    );
}
