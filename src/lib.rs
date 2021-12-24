use hyper::{Body, Method, Request, Response, StatusCode};
use std::{
    env::var,
    sync::{
        atomic::{AtomicBool, AtomicU16, Ordering},
    },
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tracing::*;

#[cfg(feature = "distributed")]
pub mod lock;

// consts
const WORKER_ID_SHIFT: u8 = 12;
const TIMESTAMP_LEFT_SHIFT: u8 = 22;
const MASK: u16 = ((1u16) << WORKER_ID_SHIFT).overflowing_sub(1).0;
// global variable :O
static COUNTER: AtomicU16 = AtomicU16::new(0);
pub static HEALTHY: AtomicBool = AtomicBool::new(false);

// these should all be initialized before serving requests so they can panic on startup rather than on first request
#[cfg(not(feature = "distributed"))]
lazy_static::lazy_static! {
    pub static ref WORKER_ID: AtomicU16 = {
        // both panics are there because the user tried to specify the env var 
        let i = match var("WORKER_ID").map(|s| s.parse::<u16>()) {
            Ok(Ok(s)) => s,
            Ok(Err(_)) => {
                panic!("non u16 value for environment variable WORKER_ID")
            },
            Err(std::env::VarError::NotPresent) => {
                warn!("environment variable \"WORKER_ID\" is not present, using default, 0");
                0
            },
            Err(e) => {
                panic!("unable to get environment variable \"WORKER_ID\" because: {}", e);
            }
        };
            
        if i >= 2u16.pow(WORKER_ID_SHIFT as u32) {
            panic!("WORKER_ID greater than the 10 bit integer limit");
        }
        AtomicU16::new(i)
    };
}

#[cfg(feature = "distributed")]
lazy_static::lazy_static! {
    pub static ref WORKER_ID: AtomicU16 = AtomicU16::new(0);
}

lazy_static::lazy_static! {
    pub static ref EPOCH: SystemTime = {
        let s = var("EPOCH")
            .expect("environment variable \"EPOCH\" is not present")
            .parse::<u64>()
            .expect("non integer environment variable \"EPOCH\"");
        if s > SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs() as u64 
        {
            panic!("environment variable \"EPOCH\" set in the future");
        }
        UNIX_EPOCH + Duration::new(s, 0)
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

#[instrument(name = "Processing Request")]
pub async fn handle_request(req: Request<Body>) -> Result<Response<Body>, hyper::Error> {
    // this could probably be better :/
    let format = match (req.method(), req.uri().path(), req.uri().query()) {
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
    
    let time = SystemTime::now()
        .duration_since(*EPOCH)
        .expect("system time running backwards")
        .as_millis() as u64;
    //get the number of seconds since the epoch
    let uuid = (time << TIMESTAMP_LEFT_SHIFT)
        | ((WORKER_ID.load(Ordering::SeqCst) as u64) << WORKER_ID_SHIFT)
        | (COUNTER.fetch_add(1, Ordering::SeqCst) & MASK) as u64;

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
