mod commands;
mod db;
mod lol;
mod markov;

use clap::Parser;
use poise::serenity_prelude as serenity;
use serde::Deserialize;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub struct Data {
    pub db: tokio_rusqlite::Connection,
    pub markov: Arc<RwLock<markov::MarkovModel>>,
    pub lol: Arc<lol::LolTracker>,
    pub config: Arc<Configuration>,
    pub started: Instant,
    pub queries: AtomicU64,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

const LOL_TICK_INTERVAL: Duration = Duration::from_secs(2);

/// Forebodere, a Discord quote bot.
#[derive(Parser)]
struct Cli {
    /// Path to the JSON configuration file.
    #[arg(long)]
    config: PathBuf,
}

#[derive(Debug, Clone, Deserialize)]
struct Reaction {
    phrase: String,
    emoji: String,
}

fn default_reactions() -> Vec<Reaction> {
    Vec::new()
}

#[derive(Debug, Clone, Deserialize)]
struct TierMessages {
    #[serde(default = "default_low_message")]
    low: String,
    #[serde(default = "default_medium_message")]
    medium: String,
    #[serde(default = "default_high_message")]
    high: String,
}

fn default_low_message() -> String {
    "Low".to_string()
}

fn default_medium_message() -> String {
    "Medium".to_string()
}

fn default_high_message() -> String {
    "High".to_string()
}

impl Default for TierMessages {
    fn default() -> Self {
        Self {
            low: default_low_message(),
            medium: default_medium_message(),
            high: default_high_message(),
        }
    }
}

impl TierMessages {
    fn get(&self, tier: lol::Tier) -> &str {
        match tier {
            lol::Tier::Low => &self.low,
            lol::Tier::Medium => &self.medium,
            lol::Tier::High => &self.high,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
pub struct Configuration {
    /// Path to the SQLite database.
    db: PathBuf,

    #[serde(default = "default_prefix")]
    prefix: String,

    #[serde(default = "default_quiet_gap_seconds")]
    lol_quiet_gap_seconds: u64,

    #[serde(default = "default_laugh_words")]
    laugh_words: Vec<String>,

    #[serde(default = "default_reactions")]
    reactions: Vec<Reaction>,

    #[serde(default)]
    lol_tier_messages: TierMessages,

    #[serde(default = "default_markov_order")]
    markov_default_order: u32,
}

fn default_prefix() -> String {
    "!".to_string()
}

fn default_quiet_gap_seconds() -> u64 {
    lol::DEFAULT_QUIET_GAP.as_secs()
}

fn default_laugh_words() -> Vec<String> {
    Vec::new()
}

fn default_markov_order() -> u32 {
    markov::DEFAULT_ORDER
}

fn load_configuration(path: &Path) -> Configuration {
    let data = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("Unable to read config file {}: {e}", path.display()));
    serde_json::from_str(&data)
        .unwrap_or_else(|e| panic!("Unable to parse config file {}: {e}", path.display()))
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let config = Arc::new(load_configuration(&cli.config));
    let prefix = config.prefix.clone();
    let token =
        std::env::var("DISCORD_TOKEN").expect("Missing `DISCORD_TOKEN` environment variable");

    let framework = poise::Framework::builder()
        .setup(move |ctx, _ready, _framework| {
            let ctx = ctx.clone();
            let config = Arc::clone(&config);
            Box::pin(async move {
                let db = tokio_rusqlite::Connection::open(&config.db).await?;
                db.call(|conn| Ok(db::init(conn)?)).await?;

                let quotes = db.call(|conn| Ok(db::all_quote_texts(conn)?)).await?;
                let order = markov::Order::new(config.markov_default_order).unwrap_or_else(|| {
                    panic!(
                        "markov_default_order must be between {} and {}",
                        markov::MIN_ORDER,
                        markov::MAX_ORDER
                    )
                });
                let model = markov::MarkovModel::build(order, &quotes);

                let lol = Arc::new(lol::LolTracker::new(
                    Duration::from_secs(config.lol_quiet_gap_seconds),
                    config.laugh_words.clone(),
                ));
                tokio::spawn(run_lol_tick(ctx, Arc::clone(&lol), Arc::clone(&config)));

                Ok(Data {
                    db,
                    markov: Arc::new(RwLock::new(model)),
                    lol,
                    config,
                    started: Instant::now(),
                    queries: AtomicU64::new(0),
                })
            })
        })
        .options(poise::FrameworkOptions {
            commands: vec![
                commands::addquote(),
                commands::quote(),
                commands::quoteall(),
                commands::markov(),
                commands::status(),
                commands::slap(),
                commands::help(),
            ],
            prefix_options: poise::PrefixFrameworkOptions {
                prefix: Some(prefix),
                ..Default::default()
            },
            pre_command: |ctx| {
                Box::pin(async move {
                    ctx.data().queries.fetch_add(1, Ordering::Relaxed);
                })
            },
            on_error: |error| Box::pin(on_error(error)),
            event_handler: |ctx, event, framework, data| {
                Box::pin(event_handler(ctx, event, framework, data))
            },
            ..Default::default()
        })
        .build();

    let intents =
        serenity::GatewayIntents::non_privileged() | serenity::GatewayIntents::MESSAGE_CONTENT;

    let mut client = serenity::ClientBuilder::new(token, intents)
        .framework(framework)
        .await
        .expect("Failed to build Discord client");

    client.start().await.expect("Client error");
}

async fn on_error(error: poise::FrameworkError<'_, Data, Error>) {
    match error {
        poise::FrameworkError::Setup { error, .. } => {
            panic!("Failed to start bot: {error:?}")
        }
        poise::FrameworkError::Command { error, ctx, .. } => {
            tracing::error!("error in command `{}`: {error:?}", ctx.command().name);
        }
        error => {
            if let Err(e) = poise::builtins::on_error(error).await {
                tracing::error!("error while handling error: {e}");
            }
        }
    }
}

async fn event_handler(
    ctx: &serenity::Context,
    event: &serenity::FullEvent,
    _framework: poise::FrameworkContext<'_, Data, Error>,
    data: &Data,
) -> Result<(), Error> {
    let serenity::FullEvent::Message { new_message } = event else {
        return Ok(());
    };

    if new_message.author.id == ctx.cache.current_user().id {
        return Ok(());
    }

    let content = new_message.content.to_lowercase();
    for reaction in &data.config.reactions {
        if content.contains(&reaction.phrase.to_lowercase()) {
            if let Some(guild_id) = new_message.guild_id {
                let emoji = ctx.cache.guild(guild_id).and_then(|guild| {
                    guild
                        .emojis
                        .values()
                        .find(|e| e.name == reaction.emoji)
                        .cloned()
                });
                if let Some(emoji) = emoji {
                    new_message.react(ctx, emoji).await?;
                }
            }
        }
    }

    data.lol.handle(
        new_message.channel_id,
        new_message.author.id,
        &new_message.content,
        Instant::now(),
    );

    Ok(())
}

async fn run_lol_tick(
    ctx: serenity::Context,
    lol: Arc<lol::LolTracker>,
    config: Arc<Configuration>,
) {
    let mut interval = tokio::time::interval(LOL_TICK_INTERVAL);
    loop {
        interval.tick().await;
        for announcement in lol.due_announcements(Instant::now()) {
            if let Some(previous) = announcement.previous_message_id {
                if let Err(e) = announcement
                    .channel
                    .delete_message(&ctx.http, previous)
                    .await
                {
                    tracing::warn!("failed to delete previous lol announcement: {e}");
                }
            }

            let text = config.lol_tier_messages.get(announcement.tier);
            match announcement.channel.say(&ctx.http, text).await {
                Ok(message) => {
                    lol.record_announcement(announcement.channel, announcement.tier, message.id)
                }
                Err(e) => tracing::error!("failed to send lol announcement: {e}"),
            }
        }
    }
}
