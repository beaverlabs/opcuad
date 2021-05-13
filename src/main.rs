use serde::{Deserialize, Serialize};
use std::io::{Read, Write};
use std::net::{Shutdown, TcpListener, TcpStream};
use std::sync::mpsc;
use std::sync::{Arc, RwLock};
use std::thread;

use opcua_client::prelude::*;

const LINE_FEED: u8 = 0x0A;

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Request {
    Connect {
        host: String,
        port: u16,
        namespace: u16,
        endpoint: Option<String>,
    },
    Read {
        node_ids: Vec<String>,
    },
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
enum Response {
    ConnectOk,
    Error { message: String },
    ReadOk { values: Vec<DataValue> },
}

struct Server {
    host: String,
    port: u16,
    namespace: u16,
    endpoint: Option<String>,
}

struct State {
    server: Option<Server>,
    session: Option<Arc<RwLock<Session>>>,
    command_sender: Option<mpsc::Sender<SessionCommand>>,
}

fn main() {
    opcua_console_logging::init();

    const DEFAULT_BIND_ADDRESS: &str = "127.0.0.1";
    const PORT: u16 = 8341;

    let bind_address = match std::env::var("BIND_ADDRESS") {
        Ok(address) => address,
        Err(_) => DEFAULT_BIND_ADDRESS.to_string(),
    };

    println!("Listening on {}:{}", bind_address, PORT);

    let listener = TcpListener::bind(format!("{}:{}", bind_address, PORT)).unwrap();

    for stream in listener.incoming() {
        match stream {
            Ok(stream) => {
                println!("New connection from {}", stream.peer_addr().unwrap());
                thread::spawn(move || {
                    handle_client(stream);
                });
            }

            Err(e) => {
                eprintln!("Error: {}", e);
            }
        }
    }
}

fn handle_client(mut stream: TcpStream) {
    let mut state = State {
        session: None,
        server: None,
        command_sender: None,
    };
    let mut buf = [0 as u8; 512];
    let mut raw_request: Vec<u8> = Vec::with_capacity(512);

    loop {
        match stream.read(&mut buf) {
            Ok(0) => {
                break;
            }
            Ok(size) => {
                let data = &buf[0..size];

                if let Some(index) = data.iter().position(|&byte| byte == LINE_FEED) {
                    let (request_end, rest) = data.split_at(index);
                    raw_request.extend_from_slice(request_end);

                    match parse_request(raw_request) {
                        Ok(request) => {
                            state = match handle_request(state, request) {
                                (Ok(response), new_state) => {
                                    handle_response(&stream, response);
                                    new_state
                                }
                                (Err(error), new_state) => {
                                    handle_error(&stream, error);
                                    new_state
                                }
                            }
                        }
                        Err(err) => {
                            eprintln!("Could not parse Request {}", err);
                        }
                    }

                    raw_request = Vec::from(rest);
                } else {
                    raw_request.extend_from_slice(data);
                }
            }
            Err(e) => {
                stream.shutdown(Shutdown::Both).unwrap();
                eprintln!("Error while reading from socket: {}", e);
                break;
            }
        }
    }

    if let Some(sender) = state.command_sender {
        sender.send(SessionCommand::Stop).unwrap();
    }

    println!("Client loop finished for {}", stream.peer_addr().unwrap());
}

fn parse_request(raw_request: Vec<u8>) -> Result<Request, String> {
    if let Ok(req) = String::from_utf8(raw_request) {
        if let Ok(request) = serde_json::from_str::<Request>(&req) {
            Ok(request)
        } else {
            Err(String::from("request is not valid"))
        }
    } else {
        Err(String::from("request is not valid utf-8"))
    }
}

fn handle_request(state: State, req: Request) -> (Result<Response, String>, State) {
    match req {
        Request::Connect {
            host,
            port,
            namespace,
            endpoint,
        } => match state.session {
            None => {
                let session = connect(&host, port, &endpoint);
                let shared = session.clone();
                let command_sender = Session::run_async(shared);
                (
                    Ok(Response::ConnectOk),
                    State {
                        command_sender: Some(command_sender),
                        server: Some(Server {
                            host,
                            port,
                            namespace,
                            endpoint,
                        }),
                        session: Some(session),
                    },
                )
            }
            Some(_) => (Err(String::from("Session already in progress")), state),
        },
        Request::Read { node_ids } => match state.session {
            None => (Err(String::from("Cannot read, no active session")), state),
            Some(ref session) => {
                let namespace = match state.server {
                    Some(ref server) => server.namespace,
                    None => 0,
                };
                let nodes: Vec<ReadValueId> = node_ids
                    .iter()
                    .map(|v| NodeId::new(namespace, v.clone()).into())
                    .collect();
                let my_session = session.clone();
                let mut the_session = my_session.write().unwrap();

                match the_session.read(&nodes) {
                    Ok(values) => (Ok(Response::ReadOk { values }), state),
                    Err(err) => (
                        Err(format!("Unable to read from OPCUA server: {}", err)),
                        state,
                    ),
                }
            }
        },
    }
}

fn connect(host: &str, port: u16, endpoint: &Option<String>) -> Arc<RwLock<Session>> {
    let endpoint = if let Some(value) = endpoint {
        value
    } else {
        ""
    };

    let url = format!("opc.tcp://{}:{}{}", host, port, endpoint);

    let mut client = ClientBuilder::new()
        .application_name("Simple Client")
        .application_uri("urn:SimpleClient")
        .session_retry_limit(3)
        .trust_server_certs(true)
        .create_sample_keypair(true)
        .single_threaded_executor()
        .client()
        .unwrap();

    client
        .connect_to_endpoint(
            (
                url.as_ref(),
                SecurityPolicy::None.to_str(),
                MessageSecurityMode::None,
                UserTokenPolicy::anonymous(),
            ),
            IdentityToken::Anonymous,
        )
        .unwrap()
}

fn handle_error(mut stream: &TcpStream, message: String) {
    let response = Response::Error { message };
    let data = serde_json::to_string(&response).unwrap() + "\n";
    stream.write_all(&data.into_bytes()).unwrap();
}

fn handle_response(mut stream: &TcpStream, response: Response) {
    let data = serde_json::to_string(&response).unwrap() + "\n";

    if let Err(e) = stream.write_all(&data.into_bytes()) {
        eprintln!("Error while writing response to socket: {}", e);
    }
}
