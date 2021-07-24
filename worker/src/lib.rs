use actix::io::SinkWrite;
use actix::*;
use awc::Client;
use log::*;
use serde::Deserialize;
use std::{
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
    cell::Cell,
};
use tokio::sync::{watch};

use futures::StreamExt;

pub mod routes;

pub mod ws;

#[derive(Clone, Debug)]
pub struct State {
    pub epoch: SystemTime, // this should be immutable
    pub proc_id: u16,
    pub worker_id: watch::Receiver<u32>,
    pub counter: Cell<u16>, //
}

#[derive(Debug)]
pub enum StateError {
    FutureEpoch,
    IoError,
}

#[derive(Deserialize, Debug)]
pub struct EnvConfig {
    pub port: Option<u16>,
    pub master_url: Option<String>,
    pub epoch: Option<u64>,
    pub worker_id: Option<u32>,
}

//TODO: error handling

use ws::{InitCommand, WorkerWsClient};

impl State {
    pub async fn new_from_url(master_url: String, epoch: u64) -> Result<(watch::Receiver<u32>, SystemTime), StateError> {
        let (tx, rx) = watch::channel(32); // max of 32 workers ( this is out of range, max 31 )

        Arbiter::spawn(async {
            let (response, framed) = match Client::new().ws(master_url).connect().await {
                Ok(s) => s,
                Err(_e) => {
                    System::current().stop();
                    panic!("failed to connect to master node, stopping");
                }
            };

            println!("{:?}", response);
            let (sink, stream) = framed.split();
            let addr = WorkerWsClient::create(|ctx| {
                WorkerWsClient::add_stream(stream, ctx);
                WorkerWsClient {
                    writer: SinkWrite::new(sink, ctx),
                    worker_id: None,
                    worker_id_channel: tx,
                    hb: Instant::now(),
                }
            });

            addr.do_send(InitCommand);
        });

        Ok((rx, UNIX_EPOCH + Duration::new(epoch, 0)))
    }

    pub fn new_from_epoch(epoch: u64, wid: Option<u32>) -> (watch::Receiver<u32>, SystemTime) {
        let (_, rx) = watch::channel(wid.unwrap_or(0)); // new from epoch just returns this
        (rx, UNIX_EPOCH + Duration::new(epoch, 0))
    }

    pub async fn new_from_ecfg(ecfg: EnvConfig) -> Result<(watch::Receiver<u32>, SystemTime), StateError> {
        match ecfg.master_url {
            Some(s) => {
                let epoch = if let Some(ep) = ecfg.epoch {
                    if ep
                        < SystemTime::now()
                            .duration_since(UNIX_EPOCH)
                            .unwrap()
                            .as_secs()
                    {
                        ep
                    } else {
                        error!("Epoch is set in the future");
                        return Err(StateError::FutureEpoch);
                    }
                } else {
                    error!("Epoch must be set in cluster mode");
                    return Err(StateError::IoError);
                };

                State::new_from_url(s, epoch).await
            }
            None => {
                let epoch = if let Some(s) = ecfg.epoch {
                    if s < SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .unwrap()
                        .as_secs()
                    {
                        s
                    } else {
                        error!("Epoch is set in the future");
                        return Err(StateError::FutureEpoch);
                    }
                } else {
                    0
                };
                Ok(State::new_from_epoch(epoch, ecfg.worker_id))
            }
        }
    }
}
