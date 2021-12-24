#![feature(cell_update)]
use log::*;
use serde::Deserialize;
use std::{
    cell::Cell,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tokio::sync::watch;
use tokio::task;

pub mod routes;

pub mod lock;

#[derive(Clone, Debug)]
pub struct State {
    pub healthy: watch::Receiver<bool>,
    pub epoch: SystemTime, // this should be immutable
    pub proc_id: watch::Receiver<u16>,
    pub worker_id: u16,
    pub counter: Cell<u16>, //
}

#[derive(Deserialize, Debug)]
pub struct EnvConfig {
    pub port: Option<u16>,
    pub redis_urls: Option<String>,
    pub epoch: Option<u64>,
    pub process_id: Option<u16>,
}

#[derive(Deserialize, Debug)]
pub enum SetupError {
    FutureEpoch,
    FailedRedisURLParse,
}

pub async fn info_from_ecfg(
    ecfg: EnvConfig,
) -> Result<(watch::Receiver<u16>, SystemTime, watch::Receiver<bool>), SetupError> {
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

    let (pid_tx, wid_rx) = watch::channel(ecfg.process_id.unwrap_or(0)); // new from epoch just returns this
    let (health_tx, health_rx) = watch::channel(false);

    if ecfg.redis_urls.is_some() {
        debug!("starting in cluster mode");

        let redis_urls: Vec<String> = ecfg
            .redis_urls
            .unwrap()
            .split(',')
            .map(String::from)
            .collect();

        debug!("Trying to spawn the manager thread");

        task::spawn_blocking(move || {
            lock::manage(pid_tx, health_tx, redis_urls);
        });

        debug!("Spawned manager thread");
    } else {
        debug!("Starting in standalone mode");
        let _ = health_tx.send(true);
    }

    Ok((wid_rx, UNIX_EPOCH + Duration::new(epoch, 0), health_rx))
}
