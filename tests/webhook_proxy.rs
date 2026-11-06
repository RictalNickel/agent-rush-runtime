//! Regression: webhook deliveries must ignore proxy settings from the environment. With a
//! proxy, the proxy resolves and connects to the webhook host itself, which bypasses the
//! address `UrlGuard` pinned and so the private-address check. This file is its own test
//! binary, so the proxy variables it sets cannot affect any other test.
use agent_rush::webhooks::{UrlGuard, deliver};
use axum::{Router, body::Bytes, routing::post};
use std::{
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};

#[tokio::test]
async fn webhook_delivery_ignores_environment_proxies() {
    // A "proxy" that counts connections and answers 200 to anything.
    let proxy = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let proxy_url = format!("http://{}", proxy.local_addr().unwrap());
    let proxied = Arc::new(AtomicUsize::new(0));
    let counter = proxied.clone();
    tokio::spawn(async move {
        loop {
            let (mut socket, _) = proxy.accept().await.unwrap();
            counter.fetch_add(1, Ordering::SeqCst);
            tokio::spawn(async move {
                let mut buf = [0u8; 4096];
                let _ = socket.read(&mut buf).await;
                let _ = socket
                    .write_all(b"HTTP/1.1 200 OK\r\ncontent-length: 0\r\nconnection: close\r\n\r\n")
                    .await;
            });
        }
    });
    // SAFETY: this binary has only this test, and no other thread reads the environment
    // while it changes (the runtime is single-threaded and no client exists yet).
    unsafe {
        for name in [
            "HTTP_PROXY",
            "http_proxy",
            "HTTPS_PROXY",
            "https_proxy",
            "ALL_PROXY",
            "all_proxy",
        ] {
            std::env::set_var(name, &proxy_url);
        }
        std::env::remove_var("NO_PROXY");
        std::env::remove_var("no_proxy");
    }

    // The real receiver on loopback (allowed only by the test-only guard).
    let (tx, mut rx) = tokio::sync::mpsc::unbounded_channel::<Bytes>();
    let receiver = Router::new().route(
        "/hook",
        post(move |body: Bytes| {
            let tx = tx.clone();
            async move {
                tx.send(body).unwrap();
                "ok"
            }
        }),
    );
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let url = format!("http://{}/hook", listener.local_addr().unwrap());
    tokio::spawn(async move { axum::serve(listener, receiver).await.unwrap() });

    // The premise: a client with default settings does go through the proxy.
    let response = reqwest::Client::new()
        .post(&url)
        .body("premise")
        .send()
        .await
        .unwrap();
    assert!(response.status().is_success());
    assert_eq!(
        proxied.load(Ordering::SeqCst),
        1,
        "the environment proxy should carry a default client's request"
    );
    assert!(rx.try_recv().is_err());

    // A webhook delivery goes straight to the checked address.
    deliver(
        UrlGuard::allow_loopback_for_tests(),
        &url,
        "whsec_test",
        br#"{"event":"turn.end"}"#,
    )
    .await
    .unwrap();
    let body = tokio::time::timeout(Duration::from_secs(10), rx.recv())
        .await
        .expect("the receiver got nothing")
        .unwrap();
    assert_eq!(&body[..], br#"{"event":"turn.end"}"#);
    assert_eq!(
        proxied.load(Ordering::SeqCst),
        1,
        "the webhook delivery went through the proxy"
    );
}
