mod jwt;

pub use jwt::unsigned_jwt;

use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt as _, AsyncWriteExt as _};
use tokio::net::TcpListener;

pub struct MockHttpServer {
    base_url: String,
    requests: Arc<Mutex<Vec<String>>>,
    handle: tokio::task::JoinHandle<()>,
}

impl MockHttpServer {
    pub async fn spawn(responses: Vec<MockResponse>) -> noloong_openai::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0").await?;
        let base_url = format!("http://{}", listener.local_addr()?);
        let requests = Arc::new(Mutex::new(Vec::new()));
        let requests_for_task = Arc::clone(&requests);
        let handle = tokio::spawn(async move {
            for response in responses {
                let (mut stream, _) = listener.accept().await.expect("mock server accept");
                let request = read_request(&mut stream).await.expect("mock server read");
                requests_for_task
                    .lock()
                    .expect("mock server request log")
                    .push(request);
                write_response(&mut stream, response)
                    .await
                    .expect("mock server write");
            }
        });
        Ok(Self {
            base_url,
            requests,
            handle,
        })
    }

    pub fn base_url(&self) -> &str {
        &self.base_url
    }

    pub async fn finish(self) -> Vec<String> {
        self.handle.await.expect("mock server join");
        self.requests
            .lock()
            .expect("mock server request log")
            .clone()
    }
}

#[derive(Clone, Debug)]
pub struct MockResponse {
    status: u16,
    content_type: &'static str,
    body: String,
}

impl MockResponse {
    pub fn json(status: u16, body: serde_json::Value) -> Self {
        Self {
            status,
            content_type: "application/json",
            body: body.to_string(),
        }
    }

    pub fn text(status: u16, body: impl Into<String>) -> Self {
        Self {
            status,
            content_type: "text/plain",
            body: body.into(),
        }
    }
}

pub async fn read_request(stream: &mut tokio::net::TcpStream) -> std::io::Result<String> {
    let mut buffer = Vec::new();
    let header_end = loop {
        let mut chunk = [0; 1024];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break buffer.len();
        }
        buffer.extend_from_slice(&chunk[..read]);
        if let Some(position) = find_header_end(&buffer) {
            break position;
        }
    };
    let header_text = String::from_utf8_lossy(&buffer[..header_end]);
    let content_length = header_text
        .lines()
        .find_map(|line| {
            line.strip_prefix("content-length:")
                .or_else(|| line.strip_prefix("Content-Length:"))
        })
        .and_then(|value| value.trim().parse::<usize>().ok())
        .unwrap_or(0);
    let body_start = header_end + 4;
    while buffer.len() < body_start + content_length {
        let mut chunk = vec![0; body_start + content_length - buffer.len()];
        let read = stream.read(&mut chunk).await?;
        if read == 0 {
            break;
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    Ok(String::from_utf8_lossy(&buffer).into_owned())
}

async fn write_response(
    stream: &mut tokio::net::TcpStream,
    response: MockResponse,
) -> std::io::Result<()> {
    let status_text = if response.status == 200 {
        "OK"
    } else {
        "Error"
    };
    let response = format!(
        "HTTP/1.1 {} {}\r\nContent-Type: {}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        status_text,
        response.content_type,
        response.body.len(),
        response.body
    );
    stream.write_all(response.as_bytes()).await
}

fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}
