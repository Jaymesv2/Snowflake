use actix_web::{middleware, web, App, HttpServer};
use log::*;
use std::io::{Error, ErrorKind};
use std::sync::{Arc, Mutex};

use std::cell::Cell;
#[derive(Clone)]
struct IdGen {
    id: Arc<Mutex<u16>>,
}

use snowflake::{
    info_from_ecfg,
    routes::{get, health},
    EnvConfig, State,
};

/// This init function loads
fn init() {
    let d = dotenv::dotenv(); // causes side effects which need to happen before env logger starts.

    env_logger::Builder::new()
        .parse_env("LOG")
        .target(env_logger::Target::Stdout)
        .init();
    // process the results of dotenv
    match d {
        Ok(_) => debug!("Loaded environment variables from .env file"),
        Err(e) => match e {
            dotenv::Error::LineParse(var, line) => debug!(
                "Failed to load .env file due to an error parsing {} a {}",
                var, line
            ),
            dotenv::Error::Io(ref e) if e.kind() == std::io::ErrorKind::NotFound => {
                debug!(".env file not found")
            }
            _ => debug!("Failed to load .env file with error: \n{:?}", e),
        },
    };
}

//todo: use websockets instead of rest so if the worker dies for some reason the server can reallocate its id
#[actix_web::main]
async fn main() -> Result<(), Error> {
    init();

    let ecfg = match envy::from_env::<EnvConfig>() {
        Ok(s) => s,
        // this shouldnt happen ever. all of the env vars are optional and use defaults if not found
        Err(e) => {
            return Err(match e {
                envy::Error::MissingValue(e) => {
                    error!("Failed to find require environment variable: {}", e);
                    Error::new(ErrorKind::NotFound, e)
                }

                envy::Error::Custom(e) => {
                    error!("Failed to parse environment with error: {}", e);
                    Error::new(ErrorKind::Other, e)
                }
            })
        }
    };

    let port: u16 = ecfg.port.unwrap_or(37550);

    // this struct is kinda jank but it works :/
    let (wid_rx, epoch, health_rx) = info_from_ecfg(ecfg).await.unwrap();

    let i: Arc<Mutex<u16>> = Arc::new(Mutex::new(0));
    let i_ref = Arc::clone(&i);
    let mut worker_id = *wid_rx.borrow();

    // waits for the websocket to update the worker id if in cluster mode.
    if worker_id == 33 {
        let _ = wid_rx.clone().changed().await.unwrap();
        worker_id = *wid_rx.borrow();
    }

    info!(
        "starting with worker id: {:?}, and epoch: {:?}",
        &worker_id, &epoch
    );

    let cpus = num_cpus::get();
    let workers = if cpus >= 32 {
        warn!("This process was allocated more logical cpus than it supports, max is 32");
        32
    } else {
        cpus
    };

    HttpServer::new(move || {
        // assigns each worker a unique id between 0 - 32
        let i2 = i_ref.clone();
        let proc_id = {
            let mut u = i2.lock().unwrap();
            *u += 1;
            *u - 1
        };
        let state = State {
            healthy: health_rx.clone(),
            proc_id: proc_id,
            worker_id: wid_rx.clone(),
            epoch: epoch.clone(),
            counter: Cell::new(0),
        };
        debug!("starting worker with proc_id: {}", &state.proc_id);
        App::new()
            .data(state)
            .wrap(middleware::Logger::default())
            .route("/", web::get().to(get))
            .route("/health", web::get().to(health))
    })
    .workers(workers)
    .bind(("0.0.0.0", port))
    .unwrap()
    .run()
    .await
}
