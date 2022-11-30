use anyhow::{anyhow, bail, Context, Result};
use axum::{
    routing::{get, post},
    Router,
};
use std::{
    future::ready,
    sync::{Arc, Mutex},
};
use tokio::{
    sync::{
        mpsc::{self, error::TryRecvError},
        oneshot,
    },
    task::JoinHandle,
};

#[tokio::main]
async fn main() -> Result<()> {
    let pa = PowerAutomate::new();
    let resp = pa
        .execute(&ServerCommand::CancelBox {
            title: "Cancel Box",
            message: "",
            time_s: 2,
        })
        .await?;
    println!("{resp:?}");
    let resp = pa
        .execute(&ServerCommand::MessageBox {
            title: "title",
            message: "message",
        })
        .await?;
    println!("{resp:?}");
    let resp = pa
        .execute(&ServerCommand::CancelBox {
            title: "Cancel Box the second",
            message: "",
            time_s: 2,
        })
        .await?;
    println!("{resp:?}");
    Ok(())
}

type ChannelData = (String, oneshot::Sender<String>);
struct ServerState {
    channel_recv: mpsc::Receiver<ChannelData>,
    oneshot: Option<oneshot::Sender<String>>,
}
struct PowerAutomate {
    handle: JoinHandle<Result<(), hyper::Error>>,
    channel_send: mpsc::Sender<ChannelData>,
}

impl PowerAutomate {
    pub fn new() -> Self {
        let (channel_send, channel_recv) = mpsc::channel(1);
        let shared = Arc::new(Mutex::new(ServerState {
            channel_recv,
            oneshot: None,
        }));
        let shared_clone = shared.clone();
        let app = Router::new()
            .route(
                "/",
                get(move || {
                    let mut state = shared.lock().unwrap();
                    let a = match state.channel_recv.try_recv() {
                        Ok((command, oneshot)) => {
                            state.oneshot = Some(oneshot);
                            command
                        }
                        Err(TryRecvError::Empty) => "".to_string(),
                        _ => unimplemented!(),
                    };
                    ready(a)
                }),
            )
            .route(
                "/",
                post(move |body: String| {
                    shared_clone
                        .lock()
                        .unwrap()
                        .oneshot
                        .take()
                        .unwrap()
                        .send(body)
                        .unwrap();
                    ready("")
                }),
            );
        let handle = tokio::spawn(
            axum::Server::bind(&"127.0.0.1:3000".parse().unwrap()).serve(app.into_make_service()),
        );
        Self {
            handle,
            channel_send,
        }
    }
    pub async fn execute<'a>(&self, command: &ServerCommand<'a>) -> Result<ServerResponse> {
        let command_str = serde_json::to_string(command).unwrap();
        let (send, recv) = oneshot::channel();
        self.channel_send.send((command_str, send)).await.unwrap();
        let resp = recv.await.unwrap();
        let patched = url_escape::decode(&resp)
            .replace("+", " ")
            .replace("\r\n", "\\n")
            .replace("False", "false")
            .replace("True", "true");
        serde_json::from_str::<Result<_, ServerError>>(&patched)
            .unwrap()
            .context("Power automate returned an error")
    }
}

#[derive(Debug, serde::Serialize)]
enum ServerCommand<'a> {
    CancelBox {
        title: &'a str,
        message: &'a str,
        time_s: usize,
    },
    MessageBox {
        title: &'a str,
        message: &'a str,
    },
}

#[derive(Debug, serde::Deserialize)]
enum ServerResponse {
    CancelBox { cancelled: bool },
    MessageBox,
}

#[derive(Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
#[error("{0}")]
struct ServerError(String);
