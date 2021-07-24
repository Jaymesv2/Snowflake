extern crate dotenv;
use actix_web::{web, App, HttpServer};

use actix::{Actor, Addr};

use log::{debug, info};
use snowflake_master::{
    health,
    id_manager::{used, IdManager},
    ws::ws,
    EnvConfig, State,
};

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

#[actix_rt::main]
async fn main() -> std::io::Result<()> {
    init();

    let ecfg = match envy::from_env::<EnvConfig>() {
        Ok(s) => s,
        Err(e) => panic!("Failed to parse environment\n{:#?}", e),
    };

    let manager: Addr<IdManager> = IdManager::new().start();

    let state = web::Data::new(State::new(manager).await);

    info!("starting on port: {port}", port = &ecfg.port);

    HttpServer::new(move || {
        App::new()
            .app_data(state.clone())
            .route("/", web::get().to(ws))
            .route("/used", web::get().to(used))
            .route("/health", web::get().to(health))
    })
    .bind(("0.0.0.0", ecfg.port as u16))?
    .run()
    .await
}
