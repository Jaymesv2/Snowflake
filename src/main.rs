extern crate tokio;
use hyper::Server;
use snowflake::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tokio::sync::Mutex;
use tokio::task;
use tower::{make::Shared, ServiceBuilder};
use tower_http::add_extension::AddExtensionLayer;
use tower_http::trace::TraceLayer;
use tracing::*;
use url::Url;

//todo: use websockets instead of rest so if the worker dies for some reason the server can reallocate its id
fn main() {
    let d = dotenv::dotenv(); // causes side effects which need to happen before env logger starts.

    /*env_logger::Builder::new()
    .parse_env("LOG")
    .target(env_logger::Target::Stdout)
    .init();*/
    // process the results of dotenv
    /*let my_subscriber = tracing_subscriber::fmt::Subscriber::default();
    tracing::subscriber::set_global_default(my_subscriber)
    .expect("setting tracing default failed");*/

    use tracing_subscriber::prelude::*;
    use tracing_subscriber::{filter::EnvFilter, fmt};

    let fmt_layer = fmt::layer().with_target(false);
    let filter_layer = EnvFilter::try_from_env("TRACE")
        .or_else(|_| EnvFilter::try_new("INFO"))
        .unwrap();

    tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .init();

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
    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
    rt.block_on(async {
        let redis_urls: Option<Vec<Url>> = std::env::var("REDIS_URLS").ok().map(|s| {
            s.split(',')
                .map(|s| Url::parse(s).expect("invalid url in environment variable\"REDIS_URLS\""))
                .collect()
        });

        if let Some(redis_urls) = redis_urls {
            debug!("starting in cluster mode");
            debug!("Trying to spawn the manager thread");
            task::spawn_blocking(move || {
                lock::manage(redis_urls);
            });
            debug!("Spawned manager thread");
        } else {
            debug!("Starting in standalone mode");
            HEALTHY.store(true, Ordering::SeqCst);
        }
        // blocks until health channel updates (it shouldnt update until its healthy)
        while !HEALTHY.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }

        info!(
            "starting with worker id: {:?}, and epoch: {:?}",
            &*WORKER_ID, &*EPOCH
        );
        let addr = ([0, 0, 0, 0], *PORT).into();
        let counter = Arc::new(Mutex::new(0u16));
        let service = ServiceBuilder::new()
            .layer(TraceLayer::new_for_http())
            .layer(AddExtensionLayer::new(counter))
            .service_fn(handle_request);

        /*let service = make_service_fn(|_| {
            let i = counter.clone();
            async { Ok::<_, hyper::Error>(service_fn(move |req| handle_request(req, i.clone()))) }
        });*/
        let server = Server::bind(&addr).serve(Shared::new(service));
        async fn shutdown_signal() {
            // Wait for the CTRL+C signal
            tokio::signal::ctrl_c()
                .await
                .expect("failed to install CTRL+C signal handler");
        }
        let graceful = server.with_graceful_shutdown(shutdown_signal());

        info!("Listening on http://{}", addr);
        graceful.await
    })
    .unwrap();
}
