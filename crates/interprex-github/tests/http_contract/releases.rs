use bytes::Bytes;
use futures_util::{TryStreamExt, stream};
use interprex::{AssetId, AssetStreamError, AssetUpload, ReleaseId, ReleasesProvider};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
    time::{Duration, timeout},
};

use super::http_fixture::{
    assert_user_request, complete_request, json_responses, provider, repository, server,
};

async fn streaming_download_server() -> (String, oneshot::Sender<()>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let (mut stream, _) = listener.accept().await.expect("accept request");
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        loop {
            let read = stream.read(&mut buffer).await.expect("read request");
            request.extend_from_slice(&buffer[..read]);
            if complete_request(&request) {
                break;
            }
        }
        stream
            .write_all(b"HTTP/1.1 200 OK\r\ncontent-type: application/octet-stream\r\ncontent-length: 11\r\nconnection: close\r\n\r\nhello ")
            .await
            .expect("write first chunk");
        receiver.await.expect("consumer read first chunk");
        stream
            .write_all(b"world")
            .await
            .expect("write second chunk");
    });
    (format!("http://{address}"), sender)
}

#[tokio::test]
async fn releases_domain_reads_by_tag_without_vendor_types_escaping() {
    let (uri, request) = server(
        "200 OK",
        "application/json",
        include_str!("../fixtures/release.json"),
    )
    .await;
    let release = provider(uri)
        .release_by_tag(&repository(), "v0.1.0")
        .await
        .expect("release");
    assert_eq!(release.tag, "v0.1.0");
    assert_user_request(
        &request.await.expect("captured request"),
        "GET /repos/civitas-forge/interprex-sandbox/releases/tags/v0.1.0 ",
    );
}

#[tokio::test]
async fn releases_domain_streams_upload_chunks_to_the_upload_host() {
    let (uri, requests) = json_responses(vec![
        r#"{"url":"{base}/releases/88","html_url":"{base}/releases/88","assets_url":"{base}/releases/88/assets","upload_url":"{base}/repos/civitas-forge/interprex-sandbox/releases/88/assets{?name,label}","id":88,"node_id":"R_kwDOExample","tag_name":"v0.1.0","target_commitish":"main","name":null,"body":null,"draft":true,"prerelease":false,"assets":[]}"#,
        r#"{"id":99,"name":"interprex.tar.gz","label":"Darwin arm64","size":11,"browser_download_url":"https://example.invalid/interprex.tar.gz"}"#,
    ])
    .await;
    let upload = AssetUpload::new(
        11,
        stream::iter([
            Ok::<_, AssetStreamError>(Bytes::from_static(b"hello ")),
            Ok(Bytes::from_static(b"world")),
        ]),
    );
    let asset = provider(uri)
        .upload_asset(
            &repository(),
            ReleaseId::new(88).expect("release id"),
            "interprex.tar.gz",
            Some("Darwin arm64"),
            upload,
        )
        .await
        .expect("upload asset");
    assert_eq!(asset.size, 11);
    let requests = requests.await.expect("captured requests");
    assert_user_request(
        &requests[0],
        "GET /repos/civitas-forge/interprex-sandbox/releases/88 ",
    );
    assert_user_request(
        &requests[1],
        "POST /repos/civitas-forge/interprex-sandbox/releases/88/assets?name=interprex%2Etar%2Egz&label=Darwin%20arm64 ",
    );
    assert!(
        requests[1]
            .to_ascii_lowercase()
            .contains("content-length: 11")
    );
    assert!(requests[1].ends_with("hello world"));
}

#[tokio::test]
async fn releases_domain_returns_download_before_the_final_chunk_arrives() {
    let (uri, continue_download) = streaming_download_server().await;
    let mut download = timeout(
        Duration::from_secs(1),
        provider(uri).download_asset(&repository(), AssetId::new(99).expect("asset id")),
    )
    .await
    .expect("download opens before the complete body arrives")
    .expect("download stream");
    let first = timeout(Duration::from_secs(1), download.try_next())
        .await
        .expect("first chunk arrives")
        .expect("first chunk read")
        .expect("first chunk exists");
    assert_eq!(first, Bytes::from_static(b"hello "));
    continue_download.send(()).expect("continue download");
    let remaining: Vec<Bytes> = download.try_collect().await.expect("remaining chunks");
    assert_eq!(remaining.concat(), b"world");
}
