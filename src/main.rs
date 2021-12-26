extern crate tokio;
use hyper::Server;
use snowflake::*;
use std::sync::atomic::Ordering;
use tower::{make::Shared, ServiceBuilder};
use tower_http::{
    trace::{DefaultOnRequest, DefaultOnResponse, TraceLayer},
    LatencyUnit,
};
use tracing::*;
use tracing_subscriber::filter::EnvFilter;

#[cfg(all(not(feature = "distributed"), not(feature = "standalone")))]
compile_error!("one or both of features distributed or standalone must be enabled");

#[cfg(feature = "distributed-trace")]
mod trace {
    use tower_http::trace::MakeSpan;
    //use opentelemetry::sdk::propagation::TextMapCompositePropagator;
    use opentelemetry::{global::get_text_map_propagator, trace::SpanKind};
    use opentelemetry_http::HeaderExtractor;
    use tracing::*;
    use tracing_opentelemetry::OpenTelemetrySpanExt;
    #[derive(Clone, Default)]
    pub struct OtelMakeSpan {}

    impl<B> MakeSpan<B> for OtelMakeSpan {
        fn make_span(&mut self, request: &hyper::Request<B>) -> Span {
            let span = info_span!(
                "Http Request",
                "otel.kind" = %SpanKind::Server,
                method = %request.method(),
                uri = %request.uri(),
                version = ?request.version(),
                headers = ?request.headers(),
            );
            let cx = get_text_map_propagator(|p| p.extract(&HeaderExtractor(request.headers())));
            span.set_parent(cx);
            span
        }
    }
}

async fn start() {
    //distributed, or both
    #[cfg(feature = "distributed")]
    {
        use tokio::task;
        use url::ParseError;
        use url::Url;
        match std::env::var("REDIS_URLS").map(|s| {
            s.split(',')
                .map(Url::parse)
                .collect::<Result<Vec<Url>, ParseError>>()
        }) {
            Ok(Ok(redis_urls)) if !redis_urls.is_empty() => {
                if redis_urls.len() > 3 {
                    warn!("using less than 3 redis instances could be bad");
                }
                debug!("starting in cluster mode");
                task::spawn_blocking(move || {
                    lock::manage(redis_urls);
                });
                debug!("Spawned manager thread");
            }
            Ok(Ok(_)) => panic!("environment variable \"REDIS_URLS\" is set but has not values"),
            Ok(Err(e)) => panic!("failed to parse redis urls with error: {}", e),
            Err(std::env::VarError::NotPresent) => {
                HEALTHY.store(true, Ordering::SeqCst);
                #[cfg(not(feature = "standalone"))]
                panic!("No redis urls provided");
            }
            Err(std::env::VarError::NotUnicode(e)) => {
                panic!(
                    "valur of environment variable\"REDIS_URLS\" could not be read because: {:?}",
                    e
                );
            }
        }
    }
    // exclusivly standalone
    #[cfg(all(not(feature = "distributed"), feature = "standalone"))]
    HEALTHY.store(true, Ordering::SeqCst);
}

async fn init_tracer() {
    use tracing_subscriber::prelude::*;
    let filter_layer = EnvFilter::try_from_env("TRACE")
        .or_else(|_| EnvFilter::try_new("INFO"))
        .unwrap();

    #[cfg(feature = "distributed-trace")]
    let otel_layer = {
        use opentelemetry::sdk::{
            trace::{self, IdGenerator, Sampler},
            Resource,
        };
        use opentelemetry::KeyValue;
        use opentelemetry::{global, sdk::propagation::TraceContextPropagator};

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

        tracing_opentelemetry::layer().with_tracer(tracer)
    };

    #[cfg(feature = "fmt-trace")]
    let fmt_layer = tracing_subscriber::fmt::layer().with_target(false);

    #[cfg(all(feature = "distributed-trace", not(feature = "fmt-trace")))]
    let subscriber = tracing_subscriber::registry()
        .with(filter_layer)
        .with(otel_layer);

    #[cfg(all(feature = "fmt-trace", not(feature = "distributed-trace")))]
    let subscriber = tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer);

    #[cfg(all(feature = "fmt-trace", feature = "distributed-trace"))]
    let subscriber = tracing_subscriber::registry()
        .with(filter_layer)
        .with(fmt_layer)
        .with(otel_layer);

    #[cfg(any(feature = "fmt-trace", feature = "distributed-trace"))]
    tracing::subscriber::set_global_default(subscriber)
        .expect("Failed to install `tracing` subscriber.");
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

        let trace_layer = TraceLayer::new_for_http()
            .on_request(DefaultOnRequest::new().level(Level::INFO))
            .on_response(
                DefaultOnResponse::new()
                    .level(Level::INFO)
                    .latency_unit(LatencyUnit::Micros),
            );

        #[cfg(feature = "distributed-trace")]
        let trace_layer = trace_layer.make_span_with(trace::OtelMakeSpan::default());

        let addr = ([0, 0, 0, 0], *PORT).into();
        let service = ServiceBuilder::new().layer(trace_layer);

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
