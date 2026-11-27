//! Native monitor commands. One worker serializes requests and owns both streams.

use std::sync::{Arc, Mutex, mpsc};
use std::thread::{self, JoinHandle, Thread};

use digi_coupled::pcm_observer::PcmSource;
use dkaudio::{AudioInput, inputs};
use tauri::State;
use tauri::async_runtime::{Sender, channel};

#[cfg(target_os = "macos")]
mod permission;
mod stereo;
mod streams;
use streams::Streams;

type Reply<T> = Sender<Result<T, String>>;

enum Command {
    Inputs(Reply<Vec<AudioInput>>),
    Start(String, Reply<String>),
    Stop(Reply<()>),
    Status(Reply<Option<String>>),
    #[cfg(test)]
    Observe(Sender<Option<std::sync::Arc<streams::StreamSignal>>>),
    Exit,
}

pub(crate) struct RemoteAudio {
    send: mpsc::Sender<Command>,
    thread: Thread,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl RemoteAudio {
    pub(crate) fn new(pcm_source: Arc<PcmSource>) -> std::io::Result<Self> {
        let (send, receive) = mpsc::channel();
        let join = thread::Builder::new()
            .name("remote-audio".into())
            .spawn(move || run(receive, pcm_source))?;
        Ok(Self {
            send,
            thread: join.thread().clone(),
            join: Mutex::new(Some(join)),
        })
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.send
            .send(command)
            .map_err(|_| "Audio monitor worker stopped".to_owned())?;
        self.thread.unpark();
        Ok(())
    }

    async fn request<T>(&self, command: impl FnOnce(Reply<T>) -> Command) -> Result<T, String> {
        let (reply, mut receive) = channel(1);
        self.send(command(reply))?;
        receive
            .recv()
            .await
            .ok_or_else(|| "Audio monitor worker stopped".to_owned())?
    }

    pub(crate) fn shutdown(&self) {
        if let Some(join) = self.join.lock().unwrap().take() {
            let _ = self.send(Command::Exit);
            let _ = join.join();
        }
    }
}

impl Drop for RemoteAudio {
    fn drop(&mut self) {
        self.shutdown();
    }
}

fn run(receive: mpsc::Receiver<Command>, pcm_source: Arc<PcmSource>) {
    let mut streams: Option<Streams> = None;
    let mut error = None;
    loop {
        if let Some(failure) = streams.as_ref().and_then(Streams::error) {
            streams = None;
            error = Some(failure);
        }
        match receive.try_recv() {
            Ok(Command::Inputs(reply)) => {
                let _ = reply.try_send(inputs());
            }
            Ok(Command::Start(input, reply)) => {
                let stopped = streams.take().map_or(Ok(()), Streams::pause);
                error = None;
                let result = stopped
                    .and_then(|()| Streams::start(&input, &pcm_source))
                    .map(|(started, name)| {
                        streams = Some(started);
                        name
                    });
                let _ = reply.try_send(result);
            }
            Ok(Command::Stop(reply)) => {
                let stopped = streams.take().map_or(Ok(()), Streams::pause);
                error = None;
                let _ = reply.try_send(stopped);
            }
            Ok(Command::Status(reply)) => {
                let _ = reply.try_send(Ok(error.clone()));
            }
            #[cfg(test)]
            Ok(Command::Observe(reply)) => {
                let _ = reply.try_send(streams.as_ref().map(Streams::signal));
            }
            Ok(Command::Exit) | Err(mpsc::TryRecvError::Disconnected) => return,
            Err(mpsc::TryRecvError::Empty) => thread::park(),
        }
    }
}

#[tauri::command]
pub(crate) async fn remote_audio_inputs(
    state: State<'_, RemoteAudio>,
) -> Result<Vec<AudioInput>, String> {
    state.request(Command::Inputs).await
}

#[tauri::command]
pub(crate) async fn remote_audio_start(
    state: State<'_, RemoteAudio>,
    input: String,
) -> Result<String, String> {
    state.request(|reply| Command::Start(input, reply)).await
}

#[tauri::command]
pub(crate) async fn remote_audio_stop(state: State<'_, RemoteAudio>) -> Result<(), String> {
    state.request(Command::Stop).await
}

#[tauri::command]
pub(crate) async fn remote_audio_status(
    state: State<'_, RemoteAudio>,
) -> Result<Option<String>, String> {
    state.request(Command::Status).await
}
