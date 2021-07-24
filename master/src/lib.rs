extern crate dotenv;
use actix::Addr;
use actix_web::{HttpResponse, Responder};
use std::collections::VecDeque;
use tokio::sync::Mutex;

use serde::Deserialize;

pub mod id_manager;
pub mod ws;

#[derive(Deserialize, Debug)]
pub struct EnvConfig {
    pub port: u16,
}

#[derive(Debug)]
pub enum InitError {}

pub struct State {
    pub ids: Mutex<VecDeque<u32>>,
    pub id_manager: Addr<id_manager::IdManager>,
    // pub leased_ids: HashMap<u32, (Ipv4Addr, i32)>,
}

//TODO: error handling
impl State {
    pub async fn new(id_manager: Addr<id_manager::IdManager>) -> State {
        let v: VecDeque<u32> = (0..1024).collect();
        State {
            ids: Mutex::new(v),
            id_manager,
        }
    }
}

// this is unfinished
pub async fn health() -> impl Responder {
    HttpResponse::Ok()
}
