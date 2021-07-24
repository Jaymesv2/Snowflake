use actix::prelude::*;
use actix::{Actor, Context, Handler};
use std::collections::HashSet;

use log::debug;
use crate::State;
use actix_web::{web, HttpRequest, HttpResponse};


const MAX_WORKERS: u32 = 32;

pub struct IdManager {
    unused_ids: HashSet<u32>,
}

impl Actor for IdManager {
    type Context = Context<Self>;
}

#[derive(Message)]
#[rtype(result = "Option<u32>")]
pub struct GetId;

#[derive(Message)]
#[rtype(result = "()")]
pub struct ReleaseId(pub u32);

#[derive(Message)]
#[rtype(result = "String")] // this returns a string because i didnt feel like implimenting the message trait on Vec<Vec<(u16, bool)>>
pub struct GetUsageMap;

pub async fn used(state: web::Data<State>, _req: HttpRequest) -> HttpResponse {
    let x = state.id_manager.send(GetUsageMap).await.unwrap();
    HttpResponse::Ok().body(x)
}

impl Handler<GetUsageMap> for IdManager {
    type Result = String;

    fn handle(&mut self, _msg: GetUsageMap, _ctx: &mut Context<Self>) -> Self::Result {
        let mut r: Vec<(u32, bool)> = Vec::new();
        for i in 0..MAX_WORKERS {
            r.push((
                i,
                if self.unused_ids.contains(&(i as u32)) {
                    false
                } else {
                    true
                },
            ));
        }
        let mut iter = r.into_iter();
        // gotta love that inline html generation
        // i did the inline generation bc i didnt fell like importing another lib just for this one debug endpoint
        let mut r2: String = String::from(
            "<html><head><style>
.on {
  height: 50px;
  width: 50px;
  background-color: red;
}
.off {
    height: 50px;
    width: 50px;
    background-color: green;
}
</style></head><body><table>",
        );
        for _ in 0..4 {
            r2.push_str("<tr>");
            for _ in 0..8 {
                let (id, on) = iter.next().unwrap();
                let x = if on {
                    format!("<th><div class=\"on\">{}</div></th>", id)
                } else {
                    format!("<th><div class=\"off\">{}</div></th>", id)
                };
                r2.push_str(&x);
            }
            r2.push_str("</tr>")
        }
        r2.push_str("</table></body></html>");
        r2
    }
}

impl Handler<GetId> for IdManager {
    type Result = Option<u32>;

    fn handle(&mut self, _msg: GetId, _ctx: &mut Context<Self>) -> Self::Result {
        let i: u32 = *self.unused_ids.iter().next()?;
        debug!("reserving id: {}", &i);
        self.unused_ids.take(&i)
    }
}

impl Handler<ReleaseId> for IdManager {
    type Result = ();

    fn handle(&mut self, msg: ReleaseId, _ctx: &mut Context<Self>) -> Self::Result {
        let id = msg.0;
        debug!("releasing id: {}", &id);
        &self.unused_ids.insert(id);
        ()
    }
}

impl IdManager {
    pub fn new() -> IdManager {
        IdManager {
            unused_ids: (0..MAX_WORKERS).collect::<HashSet<u32>>(),
        }
    }
}

