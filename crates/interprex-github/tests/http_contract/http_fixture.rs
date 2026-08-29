use std::{
    collections::BTreeMap,
    sync::{
        OnceLock,
        atomic::{AtomicU64, Ordering},
    },
};

use rand::rngs::OsRng;
use rsa::{
    RsaPrivateKey,
    pkcs8::{EncodePrivateKey, LineEnding},
};
use secrecy::{ExposeSecret, SecretString};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    sync::oneshot,
};

use interprex::Repository;
use interprex_github::{AppCredentials, GithubConfig, from_config, from_project};

fn test_app_private_key() -> SecretString {
    static PRIVATE_KEY: OnceLock<String> = OnceLock::new();
    PRIVATE_KEY
        .get_or_init(|| {
            RsaPrivateKey::new(&mut OsRng, 2048)
                .expect("generate test RSA key")
                .to_pkcs8_pem(LineEnding::LF)
                .expect("encode test RSA key")
                .to_string()
        })
        .clone()
        .into()
}

pub(super) enum ScriptedResponse {
    Json {
        status: &'static str,
        body: String,
        headers: Vec<String>,
    },
    Close,
}

impl ScriptedResponse {
    pub(super) fn json(body: impl Into<String>) -> Self {
        Self::Json {
            status: "200 OK",
            body: body.into(),
            headers: Vec::new(),
        }
    }

    pub(super) fn status(status: &'static str, body: impl Into<String>) -> Self {
        Self::Json {
            status,
            body: body.into(),
            headers: Vec::new(),
        }
    }

    pub(super) fn with_header(mut self, header: impl Into<String>) -> Self {
        if let Self::Json { headers, .. } = &mut self {
            headers.push(header.into());
        }
        self
    }
}

pub(super) async fn scripted_responses(
    responses: Vec<ScriptedResponse>,
) -> (String, oneshot::Receiver<Vec<String>>) {
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let base_uri = format!("http://{address}");
    let response_base = base_uri.clone();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for response in responses {
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
            let ScriptedResponse::Json {
                status,
                body,
                headers,
            } = response
            else {
                continue;
            };
            let body = body.replace("{base}", &response_base);
            let headers = headers
                .into_iter()
                .map(|header| header.replace("{base}", &response_base))
                .collect::<Vec<_>>()
                .join("\r\n");
            let headers = if headers.is_empty() {
                String::new()
            } else {
                format!("{headers}\r\n")
            };
            let response = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n{body}",
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
    json_responses_with_headers(bodies.into_iter().map(|body| (body, "")).collect()).await
}

pub(super) async fn json_responses_with_headers<T>(
    responses: Vec<(T, &'static str)>,
) -> (String, oneshot::Receiver<Vec<String>>)
where
    T: Into<String> + Send + 'static,
{
    let responses = responses
        .into_iter()
        .map(|(body, headers)| (body.into(), headers))
        .collect::<Vec<(String, &'static str)>>();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let base_uri = format!("http://{address}");
    let response_base = base_uri.clone();
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for (body, headers) in responses {
            let body = body.replace("{base}", &response_base);
            let headers = headers.replace("{base}", &response_base);
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
                "HTTP/1.1 200 OK\r\ncontent-type: application/json\r\n{headers}content-length: {}\r\nconnection: close\r\n\r\n{body}",
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

pub(super) async fn status_json_responses<T>(
    responses: Vec<(&'static str, T)>,
) -> (String, oneshot::Receiver<Vec<String>>)
where
    T: Into<String> + Send + 'static,
{
    let responses = responses
        .into_iter()
        .map(|(status, body)| (status, body.into()))
        .collect::<Vec<(&'static str, String)>>();
    let listener = TcpListener::bind("127.0.0.1:0")
        .await
        .expect("bind test server");
    let address = listener.local_addr().expect("local address");
    let (sender, receiver) = oneshot::channel();
    tokio::spawn(async move {
        let mut requests = Vec::with_capacity(responses.len());
        for (status, body) in responses {
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
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            stream
                .write_all(response.as_bytes())
                .await
                .expect("write response");
        }
        sender.send(requests).ok();
    });
    (format!("http://{address}"), receiver)
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

pub(super) fn app_provider(base_uri: String, app_id: u64) -> interprex_github::GithubProvider {
    from_config(GithubConfig {
        gh_token: Some(SecretString::from("transport-test-token")),
        apps: BTreeMap::from([(
            "adr-codex-review".to_owned(),
            AppCredentials {
                app_id,
                installation_id: 34,
                private_key: test_app_private_key(),
            },
        )]),
        base_uri: Some(base_uri.clone()),
        upload_uri: Some(base_uri),
    })
    .expect("app provider")
}

pub(super) async fn project_app_provider(
    base_uri: String,
    app_id: u64,
) -> interprex_github::GithubProvider {
    static NEXT_PROJECT: AtomicU64 = AtomicU64::new(0);
    let project = std::env::temp_dir().join(format!(
        "interprex-github-publication-test-{}-{}",
        std::process::id(),
        NEXT_PROJECT.fetch_add(1, Ordering::Relaxed)
    ));
    std::fs::create_dir(&project).expect("create temporary project");
    let private_key = test_app_private_key();
    let config = format!(
        "[provider.github]\nGH_TOKEN = \"transport-test-token\"\nBASE_URI = \"{base_uri}\"\nUPLOAD_URI = \"{base_uri}\"\n\n[provider.github.apps.adr-codex-review]\nAPP_ID = {app_id}\nINSTALLATION_ID = 34\nPRIVATE_KEY = '''{}'''\n",
        private_key.expose_secret()
    );
    std::fs::write(project.join(".interprex.toml"), config).expect("write project config");
    let provider = from_project(&project).await.expect("project app provider");
    std::fs::remove_dir_all(project).expect("remove temporary project");
    provider
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
