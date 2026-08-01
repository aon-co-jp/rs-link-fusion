//! `static/landing.html`を配信する最小限のHTTP/1.1サーバー(2026-07-31
//! 追加)。ダウンロード案内・電源プロファイル説明の単一静的ページのみを
//! 返す用途のため、`hyper`/`poem`等の重量級Webフレームワークへの新規
//! 依存は避け、`tokio::net::TcpListener`上に生のHTTP応答を書き込む
//! 最小実装とした(正直な開示: リクエストの厳密なパース〈ヘッダー処理・
//! keep-alive・チャンク転送等〉は行わない、単純なリクエスト行の読み取り
//! →固定レスポンスの返却のみ)。

use anyhow::{Context, Result};
use std::net::SocketAddr;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

const LANDING_HTML: &str = include_str!("../static/landing.html");

/// 待ち受けを開始し、以後すべてのHTTPリクエストに対して(パスを問わず)
/// `LANDING_HTML`を200で返し続ける。`GET /healthz`のみ`ok`を返す軽量
/// ヘルスチェック用の特別扱いとする(他プロジェクトの`/healthz`慣習を
/// 踏襲)。
pub async fn run(bind: SocketAddr) -> Result<()> {
    let listener = TcpListener::bind(bind).await.with_context(|| format!("failed to bind {bind}"))?;
    tracing::info!(%bind, "rs-link-fusion landing page server listening");
    loop {
        let (stream, peer) = listener.accept().await?;
        tokio::spawn(async move {
            if let Err(e) = handle_connection(stream).await {
                tracing::debug!(%peer, error = %e, "landing connection ended with an error");
            }
        });
    }
}

async fn handle_connection(mut stream: tokio::net::TcpStream) -> Result<()> {
    let mut buf = [0u8; 4096];
    let n = stream.read(&mut buf).await?;
    let request = String::from_utf8_lossy(&buf[..n]);
    let request_line = request.lines().next().unwrap_or("");
    let path = request_line.split_whitespace().nth(1).unwrap_or("/");

    let (status_line, content_type, body): (&str, &str, &str) = if path == "/healthz" {
        ("HTTP/1.1 200 OK", "text/plain; charset=utf-8", "ok")
    } else {
        ("HTTP/1.1 200 OK", "text/html; charset=utf-8", LANDING_HTML)
    };

    let response = format!("{status_line}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len());
    stream.write_all(response.as_bytes()).await?;
    stream.shutdown().await.ok();
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use tokio::io::AsyncWriteExt as _;
    use tokio::net::TcpStream;

    #[tokio::test]
    async fn serves_landing_html_and_healthz_over_a_real_tcp_connection() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bind = listener.local_addr().unwrap();
        drop(listener);
        let server = tokio::spawn(run(bind));
        // サーバーがbindするまで短時間ポーリングする(固定sleepより堅牢)。
        let mut stream = None;
        for _ in 0..50 {
            if let Ok(s) = TcpStream::connect(bind).await {
                stream = Some(s);
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(20)).await;
        }
        let mut stream = stream.expect("landing server did not start in time");
        stream.write_all(b"GET / HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        let mut buf = Vec::new();
        stream.read_to_end(&mut buf).await.unwrap();
        let text = String::from_utf8_lossy(&buf);
        assert!(text.starts_with("HTTP/1.1 200 OK"));
        assert!(text.contains("RS-Link-Fusion"));

        let mut stream2 = TcpStream::connect(bind).await.unwrap();
        stream2.write_all(b"GET /healthz HTTP/1.1\r\nHost: x\r\n\r\n").await.unwrap();
        let mut buf2 = Vec::new();
        stream2.read_to_end(&mut buf2).await.unwrap();
        let text2 = String::from_utf8_lossy(&buf2);
        assert!(text2.starts_with("HTTP/1.1 200 OK"));
        assert!(text2.trim_end().ends_with("ok"));

        server.abort();
    }
}
