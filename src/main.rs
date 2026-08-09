mod commands;
mod db;
mod lol;
mod markov;

use std::sync::atomic::AtomicU64;
use std::sync::{Arc, RwLock};
use std::time::Instant;

pub struct Data {
    pub db: tokio_rusqlite::Connection,
    pub markov: Arc<RwLock<markov::MarkovModel>>,
    pub lol: Arc<lol::LolTracker>,
    pub started: Instant,
    pub queries: AtomicU64,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

fn main() {
    println!("Hello, world!");
}
