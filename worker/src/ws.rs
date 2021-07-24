//! Simple websocket client.
use std::time::{Duration, Instant};

use tokio::sync::watch;

use actix::io::SinkWrite;
use actix::*;
use actix_codec::Framed;
use awc::{
    error::WsProtocolError,
    ws::{Codec, Frame, Message},
    BoxedSocket,
};
use bytes::Bytes;
use futures::stream::SplitSink;

use log::*;

use serde::{Serialize, Deserialize};

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

const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(3);
/// How long before lack of client response causes a timeout
const CLIENT_TIMEOUT: Duration = Duration::from_secs(10);

pub struct WorkerWsClient {
    pub writer: SinkWrite<Message, SplitSink<Framed<BoxedSocket, Codec>, Message>>,
    pub hb: Instant,
    pub worker_id_channel: watch::Sender<u32>,
    pub worker_id: Option<u32>,
}

impl Actor for WorkerWsClient {
    type Context = Context<Self>;

    fn started(&mut self, ctx: &mut Context<Self>) {
        // start heartbeats otherwise server will disconnect after 10 seconds
        self.hb(ctx)
    }

    fn stopped(&mut self, _: &mut Context<Self>) {
        println!("Disconnected");
        // Stop application on disconnect
        System::current().stop();
    }
}

#[derive(Message)]
#[rtype(result = "()")]
pub struct InitCommand;

impl Handler<InitCommand> for WorkerWsClient {
    type Result = ();

    fn handle(&mut self, _: InitCommand, _ctx: &mut Context<Self>) -> Self::Result {
        self.writer.write(Message::Binary(Bytes::from(
            bincode::serialize(&ClientMessage::GetId).unwrap(),
        )));
    }
}

impl WorkerWsClient {
    fn hb(&self, ctx: &mut Context<Self>) {
        ctx.run_later(HEARTBEAT_INTERVAL, |act, ctx| {
            act.writer.write(Message::Ping(Bytes::from_static(b"")));
            if Instant::now().duration_since(act.hb) > CLIENT_TIMEOUT {
                error!("connection to master timed out, stopping");

                ctx.stop();

                return;
            }
            act.hb(ctx);
        });
    }
}
/// Handle server websocket messages
impl StreamHandler<Result<Frame, WsProtocolError>> for WorkerWsClient {
    fn handle(&mut self, msg: Result<Frame, WsProtocolError>, ctx: &mut Context<Self>) {
        match msg {
            Ok(Frame::Ping(msg)) => {
                self.hb = Instant::now();
                self.writer.write(Message::Pong(msg));
            }
            Ok(Frame::Pong(_)) => {
                self.hb = Instant::now();
            }
            Ok(Frame::Binary(bin)) => {
                let m: ServerMessage = match bincode::deserialize(bin.as_ref()) {
                    Ok(s) => s,
                    Err(_) => {
                        error!("Received invalid data from master, ignoring");
                        return;
                    }
                };
                match m {
                    ServerMessage::SetWorkerId(s) => {
                        debug!("setting worker id to {}", &s);
                        self.worker_id_channel.send(s).unwrap();
                    }
                    ServerMessage::NoIdsAvailable => {
                        error!("No ids are available, Shutting down");
                        ctx.stop();
                    }
                }
            }
            Ok(Frame::Text(txt)) => {
                println!("Server: {:?}", txt)
            }
            Err(WsProtocolError::Io(e)) if e.kind() == std::io::ErrorKind::ConnectionReset => {
                ctx.stop()
            }
            _ => warn!("unhandled message, {:?}", msg),
        }
    }

    fn started(&mut self, _ctx: &mut Context<Self>) {
        println!("Connected");
    }

    fn finished(&mut self, ctx: &mut Context<Self>) {
        println!("Server disconnected");
        ctx.stop()
    }
}

impl actix::io::WriteHandler<WsProtocolError> for WorkerWsClient {}
