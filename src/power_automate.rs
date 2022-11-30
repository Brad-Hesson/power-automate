use std::{
    future::ready,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, Result};
use axum::{
    routing::{get, post},
    Router,
};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use tokio::{
    sync::{
        mpsc::{self, error::TryRecvError},
        oneshot,
    },
    task::JoinHandle,
};

type ChannelData = (String, oneshot::Sender<String>);
struct ServerState {
    channel_recv: mpsc::Receiver<ChannelData>,
    oneshot: Option<oneshot::Sender<String>>,
}
pub struct PowerAutomate {
    handle: JoinHandle<Result<(), hyper::Error>>,
    channel_send: mpsc::Sender<ChannelData>,
}

impl PowerAutomate {
    pub fn new() -> Self {
        let (channel_send, channel_recv) = mpsc::channel(1);
        let shared = Arc::new(Mutex::new(ServerState { channel_recv, oneshot: None }));
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
                    shared_clone.lock().unwrap().oneshot.take().unwrap().send(body).unwrap();
                    ready("")
                }),
            );
        let handle = tokio::spawn(
            axum::Server::bind(&"127.0.0.1:3000".parse().unwrap()).serve(app.into_make_service()),
        );
        Self { handle, channel_send }
    }
    pub async fn execute<R: DeserializeOwned>(&self, command: &impl Serialize) -> Result<R> {
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
    pub async fn cancel_box(&self, title: &str, message: &str, duration: Duration) -> Result<bool> {
        let command = json!({
            "command": "cancel_box",
            "title": title,
            "message": message,
            "time_s": duration.as_secs()
        });
        self.execute(&command).await
    }
    pub async fn message_box(&self, title: &str, message: &str) -> Result<()> {
        let command = json!({
            "command": "message_box",
            "title": title,
            "message": message
        });
        self.execute(&command).await
    }
}

#[derive(Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
#[error("{0}")]
pub struct ServerError(String);

#[test]
fn feature() {
    let out = serde_json::to_string(&()).unwrap();
    println!("{out}");
}
