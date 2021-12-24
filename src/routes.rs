use actix_web::{web, HttpResponse, Responder};
use std::time::SystemTime;

use serde::Deserialize;

// consts
const WORKER_ID_SHIFT: i64 = 12;
const PROCESS_ID_SHIFT: i64 = 17;
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
    Base64,
    Base64LE,
}

#[derive(Deserialize)]
pub struct Query {
    format: Option<Format>,
}

#[inline]
fn get_uuid(count: u16, process_id: u16, worker_id: u16, time: u64) -> u64 {
    //get the number of seconds since the epoch
    (time << TIMESTAMP_LEFT_SHIFT)
        | ((process_id as u64) << PROCESS_ID_SHIFT)
        | ((worker_id as u64) << WORKER_ID_SHIFT)
        | count as u64
}

/// Retuns the time since the epoch unless the system clock is reporting times before the epoch
fn get_time_since(epoch: SystemTime) -> Result<u64, std::time::SystemTimeError> {
    Ok(SystemTime::now().duration_since(epoch)?.as_millis() as u64)
}

// generates a unique snowflake
pub async fn get(data: web::Data<State>, web::Query(query): web::Query<Query>) -> impl Responder {
    let uuid = get_uuid(
        data
        .counter
        .update(|n| n + 1 & (((1 as u16) << WORKER_ID_SHIFT).overflowing_sub(1).0)),
        *data.proc_id.borrow(),
        data.worker_id,
        get_time_since(data.epoch).expect("system time running backwards"),
    );

    // i would rather have each Format variant return a string and use that as the string literal for format! but format! doesnt let you use a variable as the string literal,
    match query.format.unwrap_or(DEFAULT_FORMAT) {
        Format::LowerHex => format!("{:x}", uuid),
        Format::UpperHex => format!("{:X}", uuid),
        Format::Octal => format!("{:o}", uuid),
        Format::Binary => format!("{:b}", uuid),
        Format::Decimal => format!("{}", uuid),
        Format::Base64 => base64::encode(uuid.to_be_bytes()),
        Format::Base64LE => base64::encode(uuid.to_le_bytes()),
    }
}

// this is unfinished
pub async fn health() -> impl Responder {
    HttpResponse::Ok()
}
