use rand::{seq::SliceRandom, thread_rng};
use redsync::{RedisInstance, Redsync};
use std::{sync::atomic::Ordering, time::Duration, time::Instant};
use tracing::*;
use url::Url;

use crate::*;
/*
the manage function runs on its own thread and manages the lock of the uuid
in redis locks are named "SnowflakeIdMutex{id}"
*/

fn get_dlm(redis_urls: Vec<Url>) -> Result<Redsync<RedisInstance>, ()> {
    let num_of_urls: f32 = redis_urls.len() as f32;
    let instances: Vec<RedisInstance> = redis_urls
        .into_iter()
        .filter_map(|x| match RedisInstance::new(x.clone()) {
            Ok(s) => Some(s),
            Err(e) => {
                warn!(
                    "failed to connect to redis instance {} with error {}",
                    &x, e
                );
                None
            }
        })
        .collect();
    // check to make sure that a majority of the redis instances connected successfully
    if !(instances.len() >= ((num_of_urls / 2_f32).ceil() as usize) && instances.is_empty()) {
        // if it failed to connect to enough instances
        println!("bad bad bad");
        return Err(());
    }

    Ok(Redsync::new(instances))
}

fn get_redis_client(urls: Vec<Url>) -> Result<redis::Client, ()> {
    for x in urls {
        // do some more error handling here
        match redis::Client::open(x.clone()) {
            Ok(s) => return Ok(s),
            Err(_e) => warn!("invalid urls: {}", &x),
        }
    }
    Err(())
}

//TODO error handeling for when redis isnt avaliable
pub fn manage(redis_urls: Vec<Url>) {
    debug!("starting manager");
    let mut rng = thread_rng();

    let mut conn = get_redis_client(redis_urls.clone())
        .expect("failed to connect to any redis clients")
        .get_connection()
        .unwrap();

    let mut pipe = redis::pipe();
    for x in 0..1024 {
        pipe.exists(format!("SnowflakeIdMutex{}", x));
    }

    let mut unused_ids: Vec<u16> = pipe
        .query::<Vec<bool>>(&mut conn)
        .unwrap()
        .iter()
        .enumerate()
        .filter_map(|(i, b)| if !b { Some(i as u16) } else { None })
        .collect();

    // unused ids map will show available ids in a random order, the random order will be the order it will try to aquire the ids in.
    unused_ids.shuffle(&mut rng);

    let dlm = get_dlm(redis_urls).unwrap();

    let (id, mut lock) = unused_ids
        .iter()
        .find_map(
            |x| match dlm.lock(&format!("SnowflakeIdMutex{}", x), Duration::from_secs(15)) {
                Ok(s) => Some((*x, s)),
                Err(_e) => None,
            },
        )
        .expect("failed to aquire a worker id");

    let _ = WORKER_ID.store(id, Ordering::SeqCst);
    let _ = HEALTHY.store(true, Ordering::SeqCst);

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
            let _ = HEALTHY.store(false, Ordering::SeqCst);
            break;
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    }
}
