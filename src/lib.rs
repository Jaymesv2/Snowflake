use hyper::{Body, Method, Request, Response, StatusCode};
use std::{
    env::var,
    sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
        Arc,
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::*;

use tokio::sync::Mutex;

pub mod lock;

// consts
const WORKER_ID_SHIFT: i64 = 12;
const TIMESTAMP_LEFT_SHIFT: i64 = 22;

// global variable :O
// these should all be initialized before serving requests so they can panic on startup rather than on first request
lazy_static::lazy_static! {
    pub static ref WORKER_ID: AtomicU16 = {
        let i = var("WORKER_ID")
            .expect("environment variable \"WORKER_ID\" is not present")
            .parse::<u16>()
            .expect("non u16 value for environment variable WORKER_ID");
        if i >= 1024 {
            panic!("WORKER_ID greater than the 10 bit integer limit");
        }
        AtomicU16::new(i)
    };
    pub static ref HEALTHY: AtomicBool = AtomicBool::new(false);
    pub static ref EPOCH: SystemTime = {
        let s = var("EPOCH")
            .expect("environment variable \"EPOCH\" is not present")
            .parse::<u64>()
            .ok()
            .and_then(|s| {
                if s < SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_secs() as u64
                {
                    Some(s)
                } else {
                    warn!("Future Epoch, using 0 as default");
                    None
                }
            });
        if let Some(t) = s {
             UNIX_EPOCH + Duration::new(t, 0)
        } else {
            UNIX_EPOCH
        }
    };
    pub static ref PORT: u16 = var("PORT")
        .expect("environment variable \"PORT\" is not present")
        .parse::<u16>()
        .expect("non u16 value for environment variable PORT");
}

pub enum Format {
    LowerHex,
    UpperHex,
    Octal,
    Binary,
    Decimal,
    Base64,
    Base64LE,
}

pub async fn handle_request(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    let counter = req.extensions().get::<Arc<Mutex<u16>>>().unwrap();
    let format = match (req.method(), req.uri().path(), req.uri().query()) {
        // Serve some instructions at /
        (&Method::GET, "/", None) => Format::Decimal,
        (&Method::GET, "/", Some("fmt=lowerhex")) => Format::LowerHex,
        (&Method::GET, "/", Some("fmt=upperhex")) => Format::UpperHex,
        (&Method::GET, "/", Some("fmt=octal")) => Format::Octal,
        (&Method::GET, "/", Some("fmt=binary")) => Format::Binary,
        (&Method::GET, "/", Some("fmt=decimal")) => Format::Decimal,
        (&Method::GET, "/", Some("fmt=base64")) => Format::Base64,
        (&Method::GET, "/", Some("fmt=base64le")) => Format::Base64LE,
        (&Method::GET, "/", Some("fmt=base64be")) => Format::Base64,
        _ => {
            let mut not_found = Response::default();
            *not_found.status_mut() = StatusCode::NOT_FOUND;
            return Ok(not_found);
        }
    };
    // i'm not exactly sure how to do do this with atomic which kinda sucks
    let id = {
        let mut l = counter.lock().await;
        *l = (*l + 1) & (((1u16) << WORKER_ID_SHIFT).overflowing_sub(1).0);
        *l
    };
    let time = SystemTime::now()
        .duration_since(*EPOCH)
        .expect("system time running backwards")
        .as_millis() as u64;
    //get the number of seconds since the epoch
    let uuid = (time << TIMESTAMP_LEFT_SHIFT)
        | ((WORKER_ID.load(Ordering::SeqCst) as u64) << WORKER_ID_SHIFT)
        | id as u64;

    // i would rather have each Format variant return a string and use that as the string literal for format! but format! doesnt let you use a variable as the string literal,
    let b = match format {
        Format::LowerHex => format!("{:x}", uuid),
        Format::UpperHex => format!("{:X}", uuid),
        Format::Octal => format!("{:o}", uuid),
        Format::Binary => format!("{:b}", uuid),
        Format::Decimal => format!("{}", uuid),
        Format::Base64 => base64::encode(uuid.to_be_bytes()),
        Format::Base64LE => base64::encode(uuid.to_le_bytes()),
    };

    Ok(Response::builder()
        .body(Body::from(b))
        .expect("Invalid request builder configuration"))
}
