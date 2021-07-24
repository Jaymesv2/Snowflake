use actix::prelude::*;
use actix::{Actor, Addr, StreamHandler};
use actix_web::{web, Error, HttpRequest, HttpResponse};
use actix_web_actors::ws;
use std::time::{Duration, Instant};

use log::{debug, info, warn};

use futures::executor::block_on;

use crate::{
    id_manager::{self, IdManager},
    State,
};

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(3);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

use serde::{Deserialize, Serialize};
// ------------------------------------- SHARED CODE -------------------------------------
#[derive(Serialize, Deserialize)]
pub enum ServerMessage {
    SetWorkerId(u32), // if for some reason the worker needs to update its
    NoIdsAvailable,
}
// sent by client
#[derive(Serialize, Deserialize)]
pub enum ClientMessage {
    GetId,
}
// -------------------------------------             -------------------------------------

/// Define HTTP actor
pub struct WorkerWsHandler {
    hb: Instant,
    worker_id: Option<u32>,
    id_manager: Addr<IdManager>,
}

impl Actor for WorkerWsHandler {
    type Context = ws::WebsocketContext<Self>;

    fn started(&mut self, ctx: &mut Self::Context) {
        self.hb(ctx);
    }

    fn stopped(&mut self, _ctx: &mut Self::Context) {
        if let Some(s) = self.worker_id {
            self.id_manager.do_send(id_manager::ReleaseId(s));
        }
    }
}

impl WorkerWsHandler {
    fn new(id_manager: Addr<IdManager>) -> WorkerWsHandler {
        WorkerWsHandler {
            hb: Instant::now(),
            id_manager,
            worker_id: None,
        }
    }

    /// helper method that sends ping to client every second.
    ///
    /// also this method checks heartbeats from client
    fn hb(&self, ctx: &mut <Self as Actor>::Context) {
        ctx.run_interval(HEARTBEAT_INTERVAL, |act, ctx| {
            debug!("running heartbeat");
            // check client heartbeats
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                // heartbeat timed out
                info!("Websocket Client heartbeat failed, disconnecting!");

                // stop actor
                ctx.stop();

                // don't try to send a ping
                return;
            }

            ctx.ping(b"");
        });
    }
}
//send by server

/// Handler for ws::Message message
impl StreamHandler<Result<ws::Message, ws::ProtocolError>> for WorkerWsHandler {
    fn handle(&mut self, msg: Result<ws::Message, ws::ProtocolError>, ctx: &mut Self::Context) {
        match msg {
            Ok(ws::Message::Ping(msg)) => {
                self.hb = Instant::now();
                ctx.pong(&msg);
            }
            Ok(ws::Message::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(ws::Message::Text(text)) => {
                warn!("Received text data from worker: {}, ignoring", text)
            }
            Ok(ws::Message::Binary(bin)) => {
                let m: ClientMessage = match bincode::deserialize(bin.as_ref()) {
                    Ok(s) => s,
                    Err(_) => {
                        warn!("Received invalid bincode from worker, ignoring");
                        return;
                    }
                };
                match m {
                    ClientMessage::GetId => {
                        debug!("Received GetId message");
                        let r: ServerMessage = if let Some(s) =
                            block_on(self.id_manager.send(id_manager::GetId)).unwrap()
                        {
                            self.worker_id = Some(s.clone());
                            debug!("setting worker id to: {}", &s);
                            ServerMessage::SetWorkerId(s)
                        } else {
                            ServerMessage::NoIdsAvailable
                        };
                        let u: Vec<u8> = bincode::serialize(&r).unwrap();
                        ctx.binary(u);
                    }
                }
            }
            Ok(ws::Message::Close(reason)) => {
                debug!("Websocket connection closed, Disconnecting");
                ctx.close(reason);
                ctx.stop();
            }
            _ => ctx.stop(),
        }
    }
}

pub async fn ws(
    state: web::Data<State>,
    req: HttpRequest,
    stream: web::Payload,
) -> Result<HttpResponse, Error> {
    let resp = ws::start(WorkerWsHandler::new(state.id_manager.clone()), &req, stream);
    debug!("new websocket connection, response: {:?}", resp);
    resp
}
