use log::*;
use serde::Deserialize;
use std::{
    cell::Cell,
    time::{Duration, Instant, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;

pub mod routes;

#[derive(Clone, Debug)]
pub struct State {
    pub healthy: watch::Receiver<bool>,
    pub epoch: SystemTime, // this should be immutable
    pub proc_id: u16,
    pub worker_id: watch::Receiver<u32>,
    pub counter: Cell<u16>, //
}

#[derive(Deserialize, Debug)]
pub struct EnvConfig {
    pub cluster_mode: Option<bool>,
    pub redis_urls: Option<String>,
    pub port: Option<u16>,
    pub epoch: Option<u64>,
    pub worker_id: Option<u32>,
}
/*
PORT: u16
EPOCH: u64

CLUSTER_MODE: bool
   if cluster mode is true REDIS_URLS is required and WORKER_ID is ignored

env vars:

REDIS_URLS: STRING
WORKER_ID: u8

*/
#[derive(Deserialize, Debug)]
pub enum SetupError {
    FutureEpoch,
}

pub async fn info_from_ecfg(
    ecfg: EnvConfig,
) -> Result<(watch::Receiver<u32>, SystemTime, watch::Receiver<bool>), SetupError> {
    let epoch = match ecfg.epoch {
        Some(s) => {
            if s > SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_secs()
            {
                return Err(SetupError::FutureEpoch);
            } else {
                s
            }
        }
        None => {
            info!("Unspecified epoch, defaulting to 0");
            0
        }
    };

    let (tx, rx) = watch::channel(ecfg.worker_id.unwrap_or(0)); // new from epoch just returns this
    let (health_tx, health_rx) = watch::channel(false);

    if ecfg.cluster_mode.is_some() && ecfg.cluster_mode.unwrap() {
        return Ok((rx, UNIX_EPOCH + Duration::new(epoch, 0), health_rx));
    } else {
        health_tx.send(true);
        return Ok((rx, UNIX_EPOCH + Duration::new(epoch, 0), health_rx));
    }
}