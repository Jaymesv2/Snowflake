extern crate tokio;
use hyper::Server;
use snowflake::*;
use std::sync::atomic::Ordering;
use std::sync::Arc;
use tower::{make::Shared, ServiceBuilder};
use tower_http::add_extension::AddExtensionLayer;
use tower_http::{LatencyUnit,trace::{TraceLayer, DefaultMakeSpan, DefaultOnRequest, DefaultOnResponse}};
use tracing::*;
use tracing_subscriber::filter::EnvFilter;
use std::sync::atomic::AtomicU16;

#[cfg(all(not(feature = "standalone"), not(feature = "distributed")))]
compile_error!("either \"standalone\" or \"distributed\" must be enabled");

#[cfg(all(feature = "standalone", feature = "distributed"))]
compile_error!("feature \"foo\" and feature \"bar\" cannot be enabled at the same time");

#[cfg(all(feature = "fmt-trace", feature = "distributed-trace"))]
compile_error!("feature \"foo\" and feature \"bar\" cannot be enabled at the same time");

#[cfg(feature = "distributed")]
async fn start() {
    use tokio::task;
    use url::Url;
    let redis_urls: Vec<Url> = {
        let s = std::env::var("REDIS_URLS").expect("redis urls env var is not avaliable");
        s.split(',')
            .map(|s| Url::parse(s).expect("invalid url in environment variable\"REDIS_URLS\""))
            .collect()
        };
        
    if redis_urls.is_empty() {
        panic!("No redis urls provided");
    }

    if redis_urls.len() > 3 {
        warn!("using less than 3 redis instances could lead to ");
    }

    debug!("starting in cluster mode");
    debug!("Trying to spawn the manager thread");
    task::spawn_blocking(move || {
        lock::manage(redis_urls);
    });
    debug!("Spawned manager thread");
    
}

#[cfg(feature = "standalone")]
async fn start() {
    HEALTHY.store(true, Ordering::SeqCst);
}

fn filter_layer() -> EnvFilter {
    EnvFilter::try_from_env("TRACE")
        .or_else(|_| EnvFilter::try_new("INFO"))
        .unwrap()
}

#[cfg(all(not(feature = "fmt-trace"), not(feature = "distributed-trace")))]
async fn init_tracer() {}

#[cfg(feature = "fmt-trace")]
async fn init_tracer() {
    use tracing_subscriber::prelude::*;
    let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);

    tracing_subscriber::registry()
        .with(filter_layer())
        .with(fmt_layer)
        .init();
}

#[cfg(feature = "distributed-trace")]
async fn init_tracer() {
    use opentelemetry::sdk::{
        trace::{self, IdGenerator, Sampler},
        Resource,
    };
    use opentelemetry::KeyValue;
    use opentelemetry::{global, sdk::propagation::TraceContextPropagator};
    use tracing_subscriber::layer::SubscriberExt;
    use tracing_subscriber:: Registry;

    let app_name = "Snowflake";

    global::set_text_map_propagator(TraceContextPropagator::new());

    let otlp_exporter = opentelemetry_otlp::new_exporter().tonic();
    // Then pass it into pipeline builder
    let tracer = opentelemetry_otlp::new_pipeline()
        .tracing()
        .with_trace_config(
            trace::config()
                .with_sampler(Sampler::AlwaysOn)
                .with_id_generator(IdGenerator::default())
                .with_max_events_per_span(64)
                .with_max_attributes_per_span(16)
                .with_max_events_per_span(16)
                .with_resource(Resource::new(vec![KeyValue::new("service.name", app_name)])),
        )
        .with_exporter(otlp_exporter)
        .install_batch(opentelemetry::runtime::Tokio)
        .expect("Failed to install otlp exporter");

    let subscriber = Registry::default()
        .with(filter_layer())
        .with(tracing_opentelemetry::layer().with_tracer(tracer));

    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to install `tracing` subscriber.")
}

//todo: use websockets instead of rest so if the worker dies for some reason the server can reallocate its id
fn main() {
    let rt = tokio::runtime::Runtime::new().expect("failed to build tokio runtime");
    rt.block_on(async {
        #[cfg(feature = "dotenv")]
        let d = dotenv::dotenv(); // causes side effects which need to happen before env logger starts.

        init_tracer().await;

        #[cfg(feature = "dotenv")]
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

        start().await;
        // blocks until health channel updates (it shouldnt update until its healthy)
        while !HEALTHY.load(Ordering::SeqCst) {
            std::hint::spin_loop();
        }

        info!(
            "starting with worker id: {:?}, and epoch: {:?}",
            &*WORKER_ID, &*EPOCH
        );
        let addr = ([0, 0, 0, 0], *PORT).into();
        let counter = Arc::new(AtomicU16::new(0));
        let service = ServiceBuilder::new()
            .layer(
                TraceLayer::new_for_http()
                    .make_span_with(DefaultMakeSpan::new().include_headers(true))
                    .on_request(DefaultOnRequest::new().level(Level::INFO))
                    .on_response(
                        DefaultOnResponse::new()
                            .level(Level::INFO)
                            .latency_unit(LatencyUnit::Micros),
                    ),
            )
            .layer(AddExtensionLayer::new(counter));
        
        #[cfg(feature = "distributed_tracing")]
        let service = service.layer();

        let server = Server::bind(&addr).serve(Shared::new(service.service_fn(handle_request)));
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
