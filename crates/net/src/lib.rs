//! Background HTTP and WebSocket clients.
//!
//! The host is single threaded and owns the window and the JavaScript engine,
//! so every network operation runs on its own thread and reports back through
//! a channel that the host drains once per frame.

use std::collections::HashMap;
use std::net::TcpStream;
use std::sync::mpsc::{channel, Receiver, Sender, TryRecvError};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::Duration;

use tungstenite::protocol::frame::coding::CloseCode;
use tungstenite::protocol::{CloseFrame, Message};
use tungstenite::stream::MaybeTlsStream;
use tungstenite::Utf8Bytes;

pub const MAX_BODY: u64 = 64 * 1024 * 1024;
const SOCKET_POLL: Duration = Duration::from_millis(20);

#[derive(Debug, Clone)]
pub struct HttpRequest {
    pub id: u32,
    pub method: String,
    pub url: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub struct HttpResponse {
    pub id: u32,
    pub url: String,
    pub status: u16,
    pub status_text: String,
    pub headers: Vec<(String, String)>,
    pub body: Vec<u8>,
}

#[derive(Debug)]
pub enum NetworkEvent {
    Response(HttpResponse),
    FetchError { id: u32, message: String },
    SocketOpened { id: u32 },
    SocketMessage { id: u32, text: bool, data: Vec<u8> },
    SocketClosed { id: u32, code: u16, reason: String },
    SocketError { id: u32, message: String },
}

enum SocketOut {
    Text(String),
    Binary(Vec<u8>),
    Close(u16, String),
}

#[derive(Debug, thiserror::Error)]
enum FetchError {
    #[error("invalid request: {0}")]
    Request(#[from] ureq::http::Error),
    #[error("network error: {0}")]
    Http(#[from] ureq::Error),
}

pub struct Network {
    events: Receiver<NetworkEvent>,
    event_sender: Sender<NetworkEvent>,
    requests: Sender<HttpRequest>,
    sockets: Arc<Mutex<HashMap<u32, Sender<SocketOut>>>>,
}

impl Network {
    pub fn new(fetch_workers: usize) -> Network {
        let _ = rustls::crypto::ring::default_provider().install_default();
        let (request_sender, request_receiver) = channel::<HttpRequest>();
        let (event_sender, event_receiver) = channel::<NetworkEvent>();
        let queue = Arc::new(Mutex::new(request_receiver));
        let agent = Arc::new(new_agent());
        for index in 0..fetch_workers.max(1) {
            let queue = Arc::clone(&queue);
            let events = event_sender.clone();
            let agent = Arc::clone(&agent);
            let spawned = thread::Builder::new()
                .name(format!("kaplay-net-fetch-{index}"))
                .spawn(move || fetch_worker(queue, events, agent));
            if let Err(error) = spawned {
                log::error!("failed to start the fetch worker: {error}");
            }
        }
        Network {
            events: event_receiver,
            event_sender,
            requests: request_sender,
            sockets: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn fetch(&self, request: HttpRequest) -> bool {
        self.requests.send(request).is_ok()
    }

    pub fn drain(&self, limit: usize) -> Vec<NetworkEvent> {
        let mut events = Vec::new();
        while events.len() < limit {
            match self.events.try_recv() {
                Ok(event) => events.push(event),
                Err(TryRecvError::Empty) | Err(TryRecvError::Disconnected) => break,
            }
        }
        events
    }

    pub fn open_socket(&self, id: u32, url: String) {
        let (sender, receiver) = channel::<SocketOut>();
        if let Ok(mut sockets) = self.sockets.lock() {
            sockets.insert(id, sender);
        }
        let sockets = Arc::clone(&self.sockets);
        let events = self.event_sender.clone();
        let spawned = thread::Builder::new()
            .name(format!("kaplay-net-socket-{id}"))
            .spawn(move || socket_worker(id, url, receiver, events, sockets));
        if let Err(error) = spawned {
            log::error!("failed to start the socket thread: {error}");
        }
    }

    pub fn send_text(&self, id: u32, text: String) {
        self.send_to_socket(id, SocketOut::Text(text));
    }

    pub fn send_binary(&self, id: u32, data: Vec<u8>) {
        self.send_to_socket(id, SocketOut::Binary(data));
    }

    pub fn close_socket(&self, id: u32, code: u16, reason: String) {
        self.send_to_socket(id, SocketOut::Close(code, reason));
    }

    fn send_to_socket(&self, id: u32, message: SocketOut) {
        let sender = self
            .sockets
            .lock()
            .ok()
            .and_then(|sockets| sockets.get(&id).cloned());
        if let Some(sender) = sender {
            let _ = sender.send(message);
        }
    }
}

fn new_agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .http_status_as_error(false)
        .build()
        .new_agent()
}

fn fetch_worker(
    queue: Arc<Mutex<Receiver<HttpRequest>>>,
    events: Sender<NetworkEvent>,
    agent: Arc<ureq::Agent>,
) {
    loop {
        let request = match queue.lock() {
            Ok(queue) => queue.recv(),
            Err(_) => return,
        };
        let Ok(request) = request else {
            return;
        };
        let id = request.id;
        let event = match perform_fetch(&agent, request) {
            Ok(response) => NetworkEvent::Response(response),
            Err(error) => NetworkEvent::FetchError {
                id,
                message: error.to_string(),
            },
        };
        if events.send(event).is_err() {
            return;
        }
    }
}

fn perform_fetch(agent: &ureq::Agent, request: HttpRequest) -> Result<HttpResponse, FetchError> {
    let mut builder = ureq::http::Request::builder()
        .method(request.method.as_str())
        .uri(request.url.as_str());
    for (name, value) in &request.headers {
        builder = builder.header(name, value);
    }
    let outbound = builder.body(request.body)?;
    let response = agent.run(outbound)?;

    let status = response.status().as_u16();
    let status_text = response
        .status()
        .canonical_reason()
        .unwrap_or_default()
        .to_string();
    let response_headers = response
        .headers()
        .iter()
        .map(|(name, value)| {
            (
                name.as_str().to_string(),
                value.to_str().unwrap_or_default().to_string(),
            )
        })
        .collect();
    let mut body = response.into_body();
    let bytes = body.with_config().limit(MAX_BODY).read_to_vec()?;

    Ok(HttpResponse {
        id: request.id,
        url: request.url,
        status,
        status_text,
        headers: response_headers,
        body: bytes,
    })
}

fn socket_worker(
    id: u32,
    url: String,
    outgoing: Receiver<SocketOut>,
    events: Sender<NetworkEvent>,
    sockets: Arc<Mutex<HashMap<u32, Sender<SocketOut>>>>,
) {
    let handshake = tungstenite::connect(url.as_str());
    let (mut socket, _) = match handshake {
        Ok(value) => value,
        Err(error) => {
            let _ = events.send(NetworkEvent::SocketError {
                id,
                message: error.to_string(),
            });
            let _ = events.send(NetworkEvent::SocketClosed {
                id,
                code: 1006,
                reason: String::new(),
            });
            if let Ok(mut sockets) = sockets.lock() {
                sockets.remove(&id);
            }
            return;
        }
    };
    set_read_timeout(socket.get_ref(), Some(SOCKET_POLL));
    let _ = events.send(NetworkEvent::SocketOpened { id });

    let mut closing = false;
    loop {
        while let Ok(message) = outgoing.try_recv() {
            if closing {
                continue;
            }
            let result = match message {
                SocketOut::Text(text) => socket.send(Message::Text(text.into())),
                SocketOut::Binary(data) => socket.send(Message::Binary(data.into())),
                SocketOut::Close(code, reason) => {
                    closing = true;
                    let frame = CloseFrame {
                        code: CloseCode::from(code),
                        reason: Utf8Bytes::from(reason),
                    };
                    socket.send(Message::Close(Some(frame)))
                }
            };
            if let Err(error) = result {
                let _ = events.send(NetworkEvent::SocketError {
                    id,
                    message: error.to_string(),
                });
                let _ = events.send(NetworkEvent::SocketClosed {
                    id,
                    code: 1006,
                    reason: String::new(),
                });
                if let Ok(mut sockets) = sockets.lock() {
                    sockets.remove(&id);
                }
                return;
            }
        }

        match socket.read() {
            Ok(Message::Text(text)) => {
                let _ = events.send(NetworkEvent::SocketMessage {
                    id,
                    text: true,
                    data: text.as_str().as_bytes().to_vec(),
                });
            }
            Ok(Message::Binary(data)) => {
                let _ = events.send(NetworkEvent::SocketMessage {
                    id,
                    text: false,
                    data: data.to_vec(),
                });
            }
            Ok(Message::Close(frame)) => {
                let (code, reason) = frame
                    .map(|frame| (u16::from(frame.code), frame.reason.as_str().to_string()))
                    .unwrap_or((1005, String::new()));
                let _ = events.send(NetworkEvent::SocketClosed { id, code, reason });
                break;
            }
            Ok(_) => {}
            Err(tungstenite::Error::Io(error))
                if matches!(
                    error.kind(),
                    std::io::ErrorKind::WouldBlock | std::io::ErrorKind::TimedOut
                ) => {}
            Err(error) => {
                let _ = events.send(NetworkEvent::SocketError {
                    id,
                    message: error.to_string(),
                });
                let _ = events.send(NetworkEvent::SocketClosed {
                    id,
                    code: 1006,
                    reason: String::new(),
                });
                break;
            }
        }
    }

    if let Ok(mut sockets) = sockets.lock() {
        sockets.remove(&id);
    }
}

fn set_read_timeout(stream: &MaybeTlsStream<TcpStream>, timeout: Option<Duration>) {
    match stream {
        MaybeTlsStream::Plain(stream) => {
            let _ = stream.set_read_timeout(timeout);
        }
        MaybeTlsStream::Rustls(stream) => {
            let _ = stream.get_ref().set_read_timeout(timeout);
        }
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::time::Instant;

    fn wait_for_events(network: &Network, count: usize) -> Vec<NetworkEvent> {
        let deadline = Instant::now() + Duration::from_secs(10);
        let mut events = Vec::new();
        while events.len() < count && Instant::now() < deadline {
            events.extend(network.drain(16));
            if events.len() < count {
                thread::sleep(Duration::from_millis(10));
            }
        }
        events
    }

    #[test]
    fn fetches_http_responses() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (mut stream, _) = listener.accept().unwrap();
            let mut buffer = [0u8; 1024];
            let _ = stream.read(&mut buffer);
            let body = r#"{"players":["rabbit"]}"#;
            let response = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
                body.len(),
                body
            );
            stream.write_all(response.as_bytes()).unwrap();
        });

        let network = Network::new(1);
        let started = network.fetch(HttpRequest {
            id: 7,
            method: "GET".to_string(),
            url: format!("http://127.0.0.1:{port}/colyseus/characters"),
            headers: vec![("accept".to_string(), "application/json".to_string())],
            body: Vec::new(),
        });
        assert!(started);

        let events = wait_for_events(&network, 1);
        let response = events
            .iter()
            .find_map(|event| match event {
                NetworkEvent::Response(response) => Some(response),
                _ => None,
            })
            .expect("no response event");
        assert_eq!(response.id, 7);
        assert_eq!(response.status, 200);
        assert_eq!(response.status_text, "OK");
        assert_eq!(
            String::from_utf8_lossy(&response.body),
            r#"{"players":["rabbit"]}"#
        );
        assert!(response
            .headers
            .iter()
            .any(|(name, value)| name == "content-type" && value == "application/json"));

        server.join().unwrap();
    }

    #[test]
    fn reports_connection_failures() {
        let network = Network::new(1);
        network.fetch(HttpRequest {
            id: 3,
            method: "GET".to_string(),
            url: "http://127.0.0.1:1/".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        });
        let events = wait_for_events(&network, 1);
        assert!(matches!(
            events.first(),
            Some(NetworkEvent::FetchError { id: 3, .. })
        ));
    }

    #[test]
    fn rejects_invalid_requests() {
        let network = Network::new(1);
        network.fetch(HttpRequest {
            id: 4,
            method: "GE T".to_string(),
            url: "http://127.0.0.1:1/".to_string(),
            headers: Vec::new(),
            body: Vec::new(),
        });
        let events = wait_for_events(&network, 1);
        match events.first() {
            Some(NetworkEvent::FetchError { id, message }) => {
                assert_eq!(*id, 4);
                assert!(message.contains("invalid request"), "{message}");
            }
            other => panic!("unexpected event: {other:?}"),
        }
    }

    #[test]
    fn exchanges_websocket_messages() {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        let server = thread::spawn(move || {
            let (stream, _) = listener.accept().unwrap();
            let mut socket = tungstenite::accept(stream).unwrap();
            while let Ok(message) = socket.read() {
                match message {
                    Message::Text(text) => {
                        socket
                            .send(Message::Text(format!("echo:{text}").into()))
                            .unwrap();
                    }
                    Message::Binary(data) => {
                        let mut echoed = data.to_vec();
                        echoed.reverse();
                        socket.send(Message::Binary(echoed.into())).unwrap();
                    }
                    Message::Close(_) => break,
                    _ => {}
                }
            }
        });

        let network = Network::new(1);
        network.open_socket(11, format!("ws://127.0.0.1:{port}"));
        let events = wait_for_events(&network, 1);
        assert!(matches!(
            events.first(),
            Some(NetworkEvent::SocketOpened { id: 11 })
        ));

        network.send_text(11, "hello".to_string());
        let events = wait_for_events(&network, 1);
        match events.first() {
            Some(NetworkEvent::SocketMessage { id, text, data }) => {
                assert_eq!(*id, 11);
                assert!(text);
                assert_eq!(String::from_utf8_lossy(data), "echo:hello");
            }
            other => panic!("unexpected event: {other:?}"),
        }

        network.send_binary(11, vec![1, 2, 3]);
        let events = wait_for_events(&network, 1);
        match events.first() {
            Some(NetworkEvent::SocketMessage { id, text, data }) => {
                assert_eq!(*id, 11);
                assert!(!text);
                assert_eq!(data, &vec![3, 2, 1]);
            }
            other => panic!("unexpected event: {other:?}"),
        }

        network.close_socket(11, 1000, "done".to_string());
        let events = wait_for_events(&network, 2);
        assert!(events
            .iter()
            .any(|event| matches!(event, NetworkEvent::SocketClosed { id: 11, .. })));

        server.join().unwrap();
    }
}
