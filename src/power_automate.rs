use std::{
    future::ready,
    path::Path,
    rc::Rc,
    sync::{Arc, Mutex},
    time::{Duration, SystemTime},
};

use anyhow::{bail, Context, Result};
use axum::{
    routing::{get, post},
    Router,
};
use indicatif::{ProgressBar, ProgressStyle};
use serde::{de::DeserializeOwned, Serialize};
use serde_json::json;
use tokio::{
    sync::{
        mpsc::{self, error::TryRecvError},
        oneshot,
    },
    task::JoinHandle,
};

use crate::aquisition::{Aquisition, WavegenSettings};

const WAVEGEN_GAIN: f64 = 40.;
const NANONIS_WINDOW_S: f64 = 125.;
const NANONIS_WINDOW_BUFFER_S: f64 = 5.;

static mut PA_SERVER: Option<Rc<PowerAutomate>> = None;

pub struct AquisitionDriver {
    pa: Rc<PowerAutomate>,
    pkpk: Option<f64>,
    period: Option<Duration>,
    offset: Option<f64>,
    symmetry: Option<f64>,
}
impl AquisitionDriver {
    pub async fn aquire_n_waves(
        &mut self,
        settings: WavegenSettings,
        n: usize,
    ) -> Result<Aquisition> {
        let duration = settings.period * (n + 1) as u32;
        self.aquire_duration(settings, duration).await
    }
    pub async fn aquire_duration(
        &mut self,
        settings: WavegenSettings,
        duration: Duration,
    ) -> Result<Aquisition> {
        self.apply_wavegen_settings(settings).await?;
        self.start_wavegen().await?;
        let num_aqs = duration.as_secs_f64() / NANONIS_WINDOW_S;
        let window_dur = Duration::from_secs_f64(NANONIS_WINDOW_S);
        let window_buffer_dur = Duration::from_secs_f64(NANONIS_WINDOW_BUFFER_S);
        let total_dur = duration + window_buffer_dur;
        let bar = ProgressBar::new(total_dur.as_millis() as u64 / 100).with_style(
            ProgressStyle::with_template("[{eta_precise}] {bar:60.cyan/blue} {msg}")?,
        );
        // let ticker = bar.downgrade();
        // tokio::task::spawn(async move {
        //     while let Some(bar) = ticker.upgrade() {
        //         bar.inc(1);
        //         tokio::time::sleep(Duration::from_millis(100)).await;
        //     }
        // });
        let aq_end_time = SystemTime::now() + total_dur;
        let mut window_end_time = SystemTime::now() + window_dur;
        let mut aqs = vec![];
        loop {
            bar.set_message(format!("{} of {}", aqs.len() + 1, num_aqs.ceil()));
            let aq_done = loop {
                let aq_done = aq_end_time.elapsed().is_ok();
                let window_done = window_end_time.elapsed().is_ok();
                if window_done | aq_done {
                    break aq_done;
                }
                let Err(e) = aq_end_time.elapsed() else{
                    unreachable!()
                };
                bar.set_position((total_dur - e.duration()).as_millis() as u64 / 100);
                tokio::time::sleep(Duration::from_millis(1000)).await;
            };
            window_end_time = SystemTime::now() + window_dur - window_buffer_dur;
            let mut path = std::env::temp_dir();
            let fname = format!(
                "temp{}.dat",
                SystemTime::now()
                    .duration_since(SystemTime::UNIX_EPOCH)?
                    .as_secs()
            );
            path.push(fname);
            self.save_dat(&path).await?;
            while !path.exists() {
                tokio::time::sleep(Duration::from_millis(100)).await;
            }
            tokio::time::sleep(Duration::from_millis(100)).await;
            let aq = Aquisition::read_from_file(&path, settings)?;
            std::fs::remove_file(path)?;
            aqs.push(aq);
            if aq_done {
                break;
            }
        }
        bar.finish();
        let mut aq = aqs.into_iter().reduce(Aquisition::combine).unwrap();
        let i = aq.probe.len() - (duration.as_secs_f64() * 1000. / aq.sample_period_ms) as usize;
        aq.probe = aq.probe[i..].into();
        aq.current = aq.current[i..].into();
        aq.voltage = aq.voltage[i..].into();
        Ok(aq)
    }
    pub async fn start_wavegen(&self) -> Result<()> {
        self.pa.wavegen_focus().await?;
        if !self.pa.wavegen_is_running().await? {
            self.pa.wavegen_toggle_running().await?;
        }
        Ok(())
    }
    pub async fn stop_wavegen(&self) -> Result<()> {
        self.pa.wavegen_focus().await?;
        if self.pa.wavegen_is_running().await? {
            self.pa.wavegen_toggle_running().await?;
        }
        Ok(())
    }
    pub async fn set_wavegen_pkpk(&mut self, pkpk: f64) -> Result<()> {
        if self.pkpk != Some(pkpk) {
            self.pa
                .wavegen_set_amplitude(pkpk / WAVEGEN_GAIN / 2.)
                .await?;
            self.pkpk = Some(pkpk);
        }
        Ok(())
    }
    pub async fn set_wavegen_period(&mut self, period: Duration) -> Result<()> {
        if self.period != Some(period) {
            self.pa.wavegen_set_period(period.as_secs_f64()).await?;
            self.period = Some(period);
        }
        Ok(())
    }
    pub async fn set_wavegen_offset(&mut self, offset: f64) -> Result<()> {
        if self.offset != Some(offset) {
            self.pa.wavegen_set_offset(offset).await?;
            self.offset = Some(offset);
        }
        Ok(())
    }
    pub async fn set_wavegen_symmetry(&mut self, symmetry: f64) -> Result<()> {
        if self.symmetry != Some(symmetry) {
            self.pa.wavegen_set_symmetry(symmetry).await?;
            self.symmetry = Some(symmetry);
        }
        Ok(())
    }
    pub async fn apply_wavegen_settings(&mut self, settings: WavegenSettings) -> Result<()> {
        self.set_wavegen_pkpk(settings.pkpk).await?;
        self.set_wavegen_period(settings.period).await?;
        self.set_wavegen_offset(settings.offset).await?;
        self.set_wavegen_symmetry(settings.symmetry_p).await?;
        Ok(())
    }
    pub async fn save_dat(&self, path: impl AsRef<Path>) -> Result<()> {
        let fname = path.as_ref().file_name().unwrap().to_str().unwrap();
        let folder = path.as_ref().parent().unwrap().to_str().unwrap();
        if !self.pa.is_window_open("History", "").await? {
            self.pa.nanonis_open_history().await?;
        }
        self.pa.nanonis_save_history(folder, fname).await?;
        Ok(())
    }
    pub async fn new() -> Result<Self> {
        unsafe {
            if PA_SERVER.is_none() {
                PA_SERVER = Some(Rc::new(PowerAutomate::new()))
            }
        }
        let self_ = Self {
            pa: unsafe { PA_SERVER.as_ref() }.unwrap().clone(),
            pkpk: None,
            period: None,
            offset: None,
            symmetry: None,
        };
        if !self_
            .pa
            .is_window_open("WaveForms (new workspace)", "")
            .await?
        {
            bail!("Waveforms is not open")
        };
        self_.pa.wavegen_set_trapezium().await?;
        Ok(self_)
    }
}

struct PowerAutomate {
    _handle: JoinHandle<Result<(), hyper::Error>>,
    channel_send: mpsc::Sender<(String, oneshot::Sender<String>)>,
}
macro_rules! pa_fn {
    ($name:ident($($arg:ident: $typ:ty),*) -> $res:ty) => {
        async fn $name(&self, $($arg: $typ),*) -> $res{
            let command = json!({
                "command": stringify!($name),
                $(stringify!($arg): $arg),*
            });
            self.execute(&command).await
        }
    };
}
impl PowerAutomate {
    pa_fn!(wavegen_focus() -> Result<()>);
    pa_fn!(wavegen_is_running() -> Result<bool>);
    pa_fn!(wavegen_toggle_running() -> Result<()>);
    pa_fn!(wavegen_set_trapezium() -> Result<()>);
    pa_fn!(wavegen_set_period(period: f64) -> Result<()>);
    pa_fn!(wavegen_set_amplitude(amplitude: f64) -> Result<()>);
    pa_fn!(wavegen_set_offset(offset: f64) -> Result<()>);
    pa_fn!(wavegen_set_symmetry(symmetry: f64) -> Result<()>);
    pa_fn!(nanonis_save_history(folder: &str, filename: &str) -> Result<()>);
    pa_fn!(nanonis_open_history() -> Result<()>);
    pa_fn!(is_window_open(title: &str, class: &str) -> Result<bool>);
    fn new() -> Self {
        type ChannelData = (String, oneshot::Sender<String>);
        struct ServerState {
            channel_recv: mpsc::Receiver<ChannelData>,
            oneshot: Option<oneshot::Sender<String>>,
        }
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
                        e => unimplemented!("{e:?}"),
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
        let _handle = tokio::spawn(
            axum::Server::bind(&"127.0.0.1:3000".parse().unwrap()).serve(app.into_make_service()),
        );
        Self {
            _handle,
            channel_send,
        }
    }
    async fn execute<R: DeserializeOwned>(&self, command: &impl Serialize) -> Result<R> {
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

#[derive(Debug, serde::Deserialize, serde::Serialize, thiserror::Error)]
#[error("{0}")]
pub struct ServerError(String);
