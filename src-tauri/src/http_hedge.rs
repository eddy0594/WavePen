//! Races a second copy of a provider request on a fresh connection when the
//! first one has not answered in time.
//!
//! Provider connections are kept alive between dictations, and a pooled
//! connection can be dead without the client knowing. In WavePen's logs that
//! showed up as the first LLM request after ~10 idle minutes hanging until its
//! 30 s timeout while the retry answered in ~2 s (4 of 17 dictations made 9-15
//! minutes after the previous one). Racing a fresh connection bounds that wait
//! to the hedge delay instead of the full request timeout.

use std::sync::OnceLock;
use std::time::Duration;

/// A client that opens a new connection for every request, so the second copy
/// cannot land on another dead pooled connection.
fn fresh_connection_client() -> &'static reqwest::Client {
    static CLIENT: OnceLock<reqwest::Client> = OnceLock::new();
    CLIENT.get_or_init(|| {
        reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .expect("Failed to create HTTP client")
    })
}

/// Sends the request that `build` makes on `pooled`. If no response has arrived
/// after `hedge_after`, sends a second copy on a fresh connection and returns
/// whichever response arrives first; if one copy fails, the other's result is
/// returned instead.
pub async fn send_hedged(
    pooled: &reqwest::Client,
    hedge_after: Duration,
    build: impl Fn(&reqwest::Client) -> reqwest::Result<reqwest::RequestBuilder>,
) -> reqwest::Result<reqwest::Response> {
    send_hedged_with(pooled, fresh_connection_client(), hedge_after, build).await
}

async fn send_hedged_with(
    pooled: &reqwest::Client,
    fresh: &reqwest::Client,
    hedge_after: Duration,
    build: impl Fn(&reqwest::Client) -> reqwest::Result<reqwest::RequestBuilder>,
) -> reqwest::Result<reqwest::Response> {
    let first = build(pooled)?.send();
    tokio::pin!(first);
    tokio::select! {
        result = &mut first => return result,
        () = tokio::time::sleep(hedge_after) => {}
    }

    let Ok(second) = build(fresh) else {
        return first.await;
    };
    tracing::warn!(
        "No response after {:.1}s; racing a second request on a fresh connection",
        hedge_after.as_secs_f64()
    );
    let second = second.send();
    tokio::pin!(second);
    tokio::select! {
        result = &mut first => match result {
            Ok(response) => Ok(response),
            Err(_) => second.await,
        },
        result = &mut second => match result {
            Ok(response) => Ok(response),
            Err(_) => first.await,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::{TcpListener, TcpStream};
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::sync::Arc;
    use std::time::Instant;

    /// How the local test server treats one accepted connection.
    #[derive(Clone, Copy)]
    enum Reply {
        /// Reads the request and never answers, like a dead keep-alive connection.
        Never,
        After(Duration),
    }

    /// Serves HTTP/1.1 on localhost; connection `n` is handled by `replies[n]`,
    /// later ones answer at once. Returns the URL and the accepted-connection count.
    fn spawn_server(replies: Vec<Reply>) -> (String, Arc<AtomicUsize>) {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!(
            "http://{}/v1/chat/completions",
            listener.local_addr().unwrap()
        );
        let accepted = Arc::new(AtomicUsize::new(0));
        let counter = accepted.clone();
        std::thread::spawn(move || {
            let mut silent: Vec<TcpStream> = Vec::new();
            for (index, stream) in listener.incoming().enumerate() {
                let Ok(mut stream) = stream else { break };
                counter.fetch_add(1, Ordering::SeqCst);
                let mut request = [0u8; 4096];
                let _ = stream.read(&mut request);
                match replies
                    .get(index)
                    .copied()
                    .unwrap_or(Reply::After(Duration::ZERO))
                {
                    Reply::Never => silent.push(stream),
                    Reply::After(delay) => {
                        std::thread::spawn(move || {
                            std::thread::sleep(delay);
                            let _ = stream.write_all(
                                b"HTTP/1.1 200 OK\r\nContent-Length: 2\r\nConnection: close\r\n\r\nok",
                            );
                        });
                    }
                }
            }
        });
        (url, accepted)
    }

    fn closed_port_url() -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let addr = listener.local_addr().unwrap();
        drop(listener);
        format!("http://{addr}/v1/chat/completions")
    }

    fn client() -> reqwest::Client {
        reqwest::Client::builder()
            .pool_max_idle_per_host(0)
            .build()
            .unwrap()
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_stalled_request_is_answered_by_the_fresh_connection() {
        let (url, accepted) = spawn_server(vec![Reply::Never]);
        let started = Instant::now();

        let response =
            send_hedged_with(&client(), &client(), Duration::from_millis(200), |client| {
                Ok(client.post(&url).timeout(Duration::from_secs(10)))
            })
            .await
            .unwrap();

        assert_eq!(response.text().await.unwrap(), "ok");
        assert!(started.elapsed() < Duration::from_secs(5));
        assert_eq!(accepted.load(Ordering::SeqCst), 2);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_prompt_response_sends_no_second_request() {
        let (url, accepted) = spawn_server(vec![]);

        let response = send_hedged_with(&client(), &client(), Duration::from_secs(5), |client| {
            Ok(client.post(&url))
        })
        .await
        .unwrap();

        assert_eq!(response.text().await.unwrap(), "ok");
        assert_eq!(accepted.load(Ordering::SeqCst), 1);
    }

    #[tokio::test(flavor = "multi_thread")]
    async fn a_failed_second_copy_still_waits_for_the_slow_first_one() {
        let (url, _accepted) = spawn_server(vec![Reply::After(Duration::from_millis(400))]);
        let unreachable = closed_port_url();
        let pooled = client();

        let response = send_hedged_with(&pooled, &client(), Duration::from_millis(50), |client| {
            let target = if std::ptr::eq(client, &pooled) {
                &url
            } else {
                &unreachable
            };
            Ok(client.post(target))
        })
        .await
        .unwrap();

        assert_eq!(response.text().await.unwrap(), "ok");
    }
}
