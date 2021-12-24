extern crate tokio;

use actix_web::{
    middleware,
    web::{self, Data},
    App, HttpServer,
};
use log::*;
use std::io::{Error, ErrorKind};
use std::sync::{Arc, Mutex};

use std::cell::Cell;

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

    let port: u16 = if let Some(s) = &ecfg.port { *s } else { 80 };

    let (pid_rx, epoch, health_rx) = info_from_ecfg(ecfg).await.unwrap();
    let i: Arc<Mutex<u16>> = Arc::new(Mutex::new(0));
    let i_ref = Arc::clone(&i);
    let healthy = *health_rx.borrow();

    let workers = if num_cpus::get() >= 32 {
        warn!("This process was allocated more logical cpus than it supports, max is 32");
        32
    } else {
        num_cpus::get()
    };

    // blocks until health channel updates (it shouldnt update until its healthy)
    if !healthy {
        let _ = health_rx.clone().changed().await.unwrap();
    }

    info!(
        "starting with process id: {:?}, {} workers, and epoch: {:?}",
        pid_rx.borrow(),
        &workers,
        &epoch
    );

    HttpServer::new(move || {
        // assigns each worker a unique id between 0 - 32
        let i2 = i_ref.clone();
        let worker_id = {
            let mut u = i2.lock().unwrap();
            *u += 1;
            *u - 1
        };

        let state = State {
            healthy: health_rx.clone(),
            proc_id: pid_rx.clone(),
            worker_id,
            epoch,
            counter: Cell::new(0),
        };

        debug!("starting worker id: {}", &state.worker_id);
        App::new()
            .app_data(Data::new(state))
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
