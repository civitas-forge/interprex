use secrecy::SecretString;
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

use interprex::Repository;
use interprex_github::{GithubConfig, from_config};

pub(super) async fn server(
    status: &'static str,
    content_type: &'static str,
    body: &'static str,
) -> (String, oneshot::Receiver<String>) {
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
            if read == 0 {
                break;
            }
            request.extend_from_slice(&buffer[..read]);
            if complete_request(&request) {
                break;
            }
        }
        let response = format!(
            "HTTP/1.1 {status}\r\ncontent-type: {content_type}\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
            body.len()
        );
        stream
            .write_all(response.as_bytes())
            .await
            .expect("write response");
        sender
            .send(String::from_utf8(request).expect("request is UTF-8"))
            .ok();
    });
    (format!("http://{address}"), receiver)
}

pub(super) async fn rest_pages(
    route: &'static str,
    bodies: Vec<&'static str>,
) -> (String, oneshot::Receiver<Vec<String>>) {
    rest_filtered_pages(route, "", bodies).await
}

/// Serves paginated REST responses whose next link repeats `filters`, the way
/// GitHub carries a query's filters onto its later pages. `filters` is either
/// empty or a query fragment ending in `&`.
pub(super) async fn rest_filtered_pages(
    route: &'static str,
    filters: &'static str,
    bodies: Vec<&'static str>,
) -> (String, oneshot::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let base_uri = format!("http://{address}");
    let next_base = base_uri.clone();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let page_count = bodies.len();
        let mut requests = Vec::with_capacity(page_count);
        for (index, body) in bodies.into_iter().enumerate() {
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await.expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if complete_request(&request) {
                    break;
                }
            }
            requests.push(String::from_utf8(request).expect("request is UTF-8"));
            let link = if index + 1 < page_count {
                format!(
                    "link: <{next_base}{route}?{filters}per_page=100&page={}>; rel=\"next\"\r\n",
                    index + 2
                )
            } else {
                String::new()
            };
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n{link}content-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        }
        sender.send(requests).ok();
    });
    (base_uri, receiver)
}

pub(super) async fn json_responses<T>(bodies: Vec<T>) -> (String, oneshot::Receiver<Vec<String>>)
where
    T: Into<String> + Send + 'static,
{
    let bodies = bodies.into_iter().map(Into::into).collect::<Vec<String>>();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let base_uri = format!("http://{address}");
    let response_base = base_uri.clone();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let mut requests = Vec::with_capacity(bodies.len());
        for body in bodies {
            let body = body.replace("{base}", &response_base);
            let (mut stream, _) = listener.accept().await.expect("accept request");
            let mut request = Vec::new();
            let mut buffer = [0_u8; 4096];
            loop {
                let read = stream.read(&mut buffer).await.expect("read request");
                if read == 0 {
                    break;
                }
                request.extend_from_slice(&buffer[..read]);
                if complete_request(&request) {
                    break;
                }
            }
            requests.push(String::from_utf8(request).expect("request is UTF-8"));
            let response = format!(
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        }
        sender.send(requests).ok();
    });
    (base_uri, receiver)
}

pub(super) fn complete_request(request: &[u8]) -> bool {
    let text = String::from_utf8_lossy(request);
    let Some((headers, body)) = text.split_once("\r\n\r\n") else {
        return false;
    };
    let length = headers
        .lines()
        .find_map(|line| {
            line.to_ascii_lowercase()
                .strip_prefix("content-length: ")
                .map(str::to_owned)
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    body.len() >= length
}

pub(super) fn provider(base_uri: String) -> interprex_github::GithubProvider {
    from_config(GithubConfig {
        gh_token: Some(SecretString::from("transport-test-token")),
        base_uri: Some(base_uri.clone()),
        upload_uri: Some(base_uri),
        ..GithubConfig::default()
    })
    .expect("provider")
}

pub(super) fn repository() -> Repository {
    Repository::new("civitas-forge", "interprex-sandbox").expect("repository")
}

pub(super) fn assert_user_request(request: &str, method_and_path: &str) {
    assert!(request.starts_with(method_and_path), "{request}");
    assert!(
        request
            .to_ascii_lowercase()
            .contains("authorization: bearer transport-test-token"),
        "{request}"
    );
}
