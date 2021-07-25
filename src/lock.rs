extern crate rand;

use tokio::sync::watch;

use redsync::{RedisInstance, Redsync};
use std::error::Error;
use std::time::Duration;
//use redis::Commands;
use log::*;
use rand::seq::SliceRandom;
use std::time::Instant;
/*

the manage function runs on its own thread and manages the lock of the uuid


in redis locks are named "SnowflakeIdMutex{id}"
*/
use rand::thread_rng;

fn get_dlm(redis_urls: Vec<String>) -> Result<Redsync<RedisInstance>, Box<dyn Error>> {
    Ok(Redsync::new(
        redis_urls
            .iter()
            .map(|x| RedisInstance::new(&**x).expect("bad bad bad bad bad bad"))
            .collect::<Vec<RedisInstance>>(),
    ))
}
//TODO error handeling for when redis isnt avaliable
pub fn manage(wid_tx: watch::Sender<u32>, health_tx: watch::Sender<bool>, redis_urls: Vec<String>) {
    debug!("starting manager");
    let mut rng = thread_rng();

    let mut conn = get_redis_client(redis_urls.clone())
        .expect("failed to connect to any redis clients")
        .get_connection()
        .unwrap();

    let mut pipe = redis::pipe();
    for x in 0..32 {
        pipe.exists(format!("SnowflakeIdMutex{}", x));
    }

    //let mut unused_ids: Vec<u32> = Vec::new();
    let mut unused_ids: Vec<u32> = pipe
        .query::<Vec<bool>>(&mut conn)
        .unwrap()
        .iter()
        .enumerate()
        .filter(|x| !x.1)
        .map(|(x, _)| x as u32)
        .collect();

    // unused ids map will show available ids in a random order, the random order will be the order it will try to aquire the ids in.
    unused_ids.shuffle(&mut rng);

    let dlm = get_dlm(redis_urls).unwrap();
    //let id = *unused_ids.iter().next().unwrap();

    let mut lock: redsync::Lock = redsync::Lock {
        resource: String::new(),
        value: String::new(),
        ttl: Duration::from_secs(1),
        expiry: Instant::now(),
    };
    let mut id = 25555;

    for x in unused_ids {
        let s = format!("SnowflakeIdMutex{}", x);
        lock = match dlm.lock(&s, Duration::from_secs(15)) {
            Ok(s) => {
                id = x;
                s
            }
            Err(_e) => continue,
        };
        break;
    }
    // if it didnt get an id
    if id == 25555 {
        panic!("failed to aquire a lock on an id");
    };

    let _ = wid_tx.send(id);
    let _ = health_tx.send(true);

    loop {
        let x = lock
            .expiry
            .saturating_duration_since(Instant::now())
            .as_secs();
        if x != 0 {
            if x <= 5 {
                lock = dlm.extend(&lock, Duration::from_secs(15)).unwrap();
            }
        } else {
            println!("lost lock");
            let _ = health_tx.send(false);
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}

//tries all redis urls
// () indicates none were reachable
fn get_redis_client(urls: Vec<String>) -> Result<redis::Client, ()> {
    for x in urls {
        // do some more error handling here
        match redis::Client::open(x.clone()) {
            Ok(s) => return Ok(s),
            Err(_e) => warn!("invalid urls: {}", &x),
        }
    }
    Err(())
}

//fn check_available_ids()
