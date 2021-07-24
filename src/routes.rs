use actix_web::{web, HttpResponse, Responder};
use std::time::SystemTime;

use serde::Deserialize;

// consts
const WORKER_ID_SHIFT: i64 = 12;
const DATACENTER_ID_SHIFT: i64 = 17;
const TIMESTAMP_LEFT_SHIFT: i64 = 22;

const DEFAULT_FORMAT: Format = Format::Decimal;

use crate::State;

#[derive(Deserialize)]
pub enum Format {
    LowerHex,
    UpperHex,
    Octal,
    Binary,
    Decimal,
}

#[derive(Deserialize)]
pub struct Query {
    format: Option<Format>,
}

fn get_uuid(count: u16, datacenter_id: u16, worker_id: u32, time: u64) -> u64 {
    //get the number of seconds since the epoch
    (time << TIMESTAMP_LEFT_SHIFT)
        | ((datacenter_id as u64) << DATACENTER_ID_SHIFT)
        | ((worker_id as u64) << WORKER_ID_SHIFT)
        | count as u64
}
/// Retuns the time since the epoch unless the system clock is reporting times before the epoch
fn get_time_since(epoch: SystemTime) -> Result<u64, std::time::SystemTimeError> {
    let t = SystemTime::now().duration_since(epoch)?;
    Ok((t.as_secs() * 1000) as u64 + t.subsec_millis() as u64)
}

// generates a unique snowflake
pub async fn get(data: web::Data<State>, web::Query(query): web::Query<Query>) -> impl Responder {
    //pub async fn get(data: &mut State, web::Query(query): web::Query<Query>) -> impl Responder {
    //let r = *std::sync::Arc::get_mut(&mut data).unwrap();
    let n = data.counter.get();
    if n >= 4095 {
        data.counter.set(0);
    } else {
        data.counter.set(n + 1)
    }

    let uuid = get_uuid(
        n,
        data.proc_id,
        *data.worker_id.borrow(),
        get_time_since(data.epoch).expect("system time running backwards"),
    );

    // i would rather have each Format variant return a string and use that as the string literal for format! but format! doesnt let you use a variable as the string literal,
    match query.format.unwrap_or(DEFAULT_FORMAT) {
        Format::LowerHex => format!("{:x}", uuid),
        Format::UpperHex => format!("{:X}", uuid),
        Format::Octal => format!("{:o}", uuid),
        Format::Binary => format!("{:b}", uuid),
        Format::Decimal => format!("{}", uuid),
    }
}

// this is unfinished
pub async fn health() -> impl Responder {
    HttpResponse::Ok()
}
