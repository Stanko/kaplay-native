use std::io::{Read, Write};
use std::net::{TcpListener, TcpStream};
use std::sync::Arc;
use std::thread;
use std::time::{Duration, Instant};

use criterion::{criterion_group, criterion_main, Criterion, Throughput};
use kaplay_native_net::{HttpRequest, HttpResponse, Network, NetworkEvent};

const HTTP_SIZES: [usize; 3] = [4096, 65536, 262144];
const WS_BATCH: usize = 64;

fn spawn_http_server(body: Arc<Vec<u8>>) -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("http listener");
    let port = listener.local_addr().expect("addr").port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let body = Arc::clone(&body);
            thread::spawn(move || serve_http(&mut stream, &body));
        }
    });
    port
}

fn serve_http(stream: &mut TcpStream, body: &[u8]) {
    let _ = stream.set_nodelay(true);
    let mut buffer = [0u8; 8192];
    loop {
        let mut request = Vec::new();
        loop {
            let read = match stream.read(&mut buffer) {
                Ok(0) | Err(_) => return,
                Ok(read) => read,
            };
            request.extend_from_slice(&buffer[..read]);
            if request.windows(4).any(|window| window == b"\r\n\r\n") {
                break;
            }
            if request.len() > 64 * 1024 {
                return;
            }
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: keep-alive\r\n\r\n",
            body.len()
        );
        let mut response = head.into_bytes();
        response.extend_from_slice(body);
        if stream.write_all(&response).is_err() {
            return;
        }
    }
}

fn spawn_ws_echo_server() -> u16 {
    let listener = TcpListener::bind("127.0.0.1:0").expect("ws listener");
    let port = listener.local_addr().expect("addr").port();
    thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            thread::spawn(move || {
                let _ = stream.set_nodelay(true);
                let mut socket = match tungstenite::accept(stream) {
                    Ok(socket) => socket,
                    Err(error) => {
                        eprintln!("echo server accept error: {error:?}");
                        return;
                    }
                };
                loop {
                    match socket.read() {
                        Ok(message) => {
                            let closing = message.is_close();
                            if message.is_text() || message.is_binary() {
                                let _ = socket.send(message);
                            }
                            if closing {
                                return;
                            }
                        }
                        Err(_) => return,
                    }
                }
            });
        }
    });
    port
}

fn http_request(id: u32, url: &str) -> HttpRequest {
    HttpRequest {
        id,
        method: "GET".to_string(),
        url: url.to_string(),
        headers: Vec::new(),
        body: Vec::new(),
    }
}

fn wait_for_response(network: &Network, id: u32) -> HttpResponse {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        for event in network.drain(1) {
            if let NetworkEvent::Response(response) = event {
                if response.id == id {
                    return response;
                }
            }
        }
        assert!(Instant::now() < deadline, "fetch {id} timed out");
        thread::sleep(Duration::from_micros(20));
    }
}

fn fetch_roundtrip(network: &Network, id: u32, url: &str) -> usize {
    assert!(network.fetch(http_request(id, url)), "fetch submitted");
    wait_for_response(network, id).body.len()
}

fn http(c: &mut Criterion) {
    let network = Network::new(4);
    let mut group = c.benchmark_group("net/http/roundtrip");
    for size in HTTP_SIZES {
        let port = spawn_http_server(Arc::new(vec![0x5a; size]));
        let url = format!("http://127.0.0.1:{port}/body.bin");
        group.throughput(Throughput::Bytes(size as u64));
        group.bench_with_input(format!("{size}"), &url, |b, url| {
            let mut id = 1u32;
            id += 1;
            fetch_roundtrip(&network, id, url);
            b.iter(|| {
                id += 1;
                fetch_roundtrip(&network, id, url);
            });
        });
    }
    group.finish();

    let port = spawn_http_server(Arc::new(vec![0x5a; 65536]));
    let url = format!("http://127.0.0.1:{port}/body.bin");
    let mut id = 1_000_000u32;
    c.bench_function("net/http/submit", |b| {
        b.iter(|| {
            id += 1;
            network.fetch(http_request(id, &url));
        });
    });
}

fn wait_for_message(network: &Network, socket: u32) -> usize {
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        for event in network.drain(1) {
            if let NetworkEvent::SocketMessage { id, data, .. } = event {
                if id == socket {
                    return data.len();
                }
            }
        }
        assert!(Instant::now() < deadline, "socket {socket} timed out");
        thread::sleep(Duration::from_micros(20));
    }
}

fn open_socket(network: &Network, socket: u32, url: &str) {
    network.open_socket(socket, url.to_string());
    let deadline = Instant::now() + Duration::from_secs(10);
    loop {
        for event in network.drain(32) {
            if let NetworkEvent::SocketOpened { id } = event {
                if id == socket {
                    return;
                }
            }
        }
        assert!(Instant::now() < deadline, "socket {socket} open timed out");
        thread::sleep(Duration::from_micros(20));
    }
}

fn websocket(c: &mut Criterion) {
    let network = Network::new(4);
    let port = spawn_ws_echo_server();
    let url = format!("ws://127.0.0.1:{port}/echo");
    open_socket(&network, 1, &url);
    let payload = "benchmark".to_string();

    let mut group = c.benchmark_group("net/ws");
    network.send_text(1, payload.clone());
    wait_for_message(&network, 1);
    group.throughput(Throughput::Elements(1));
    group.bench_function("echo-roundtrip", |b| {
        b.iter(|| {
            network.send_text(1, payload.clone());
            wait_for_message(&network, 1);
        });
    });

    group.throughput(Throughput::Elements(WS_BATCH as u64));
    group.bench_function("echo-batch-64", |b| {
        b.iter(|| {
            for _ in 0..WS_BATCH {
                network.send_text(1, payload.clone());
            }
            for _ in 0..WS_BATCH {
                wait_for_message(&network, 1);
            }
        });
    });
    group.finish();
}

criterion_group!(benches, http, websocket);
criterion_main!(benches);
