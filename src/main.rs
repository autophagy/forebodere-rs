mod commands;
mod db;
mod lol;
mod markov;

use clap::Parser;
use poise::serenity_prelude as serenity;
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, RwLock};
use std::time::{Duration, Instant};

pub struct Data {
    pub db: tokio_rusqlite::Connection,
    pub markov: Arc<RwLock<markov::MarkovModel>>,
    pub lol: Arc<lol::LolTracker>,
    pub started: Instant,
    pub queries: AtomicU64,
}

pub type Error = Box<dyn std::error::Error + Send + Sync>;
pub type Context<'a> = poise::Context<'a, Data, Error>;

const LOL_TICK_INTERVAL: Duration = Duration::from_secs(2);

/// Forebodere, a Discord quote bot.
#[derive(Parser)]
struct Cli {
    /// Path to the SQLite database.
    #[arg(long)]
    db: PathBuf,

    /// Command prefix.
    #[arg(long, default_value = "!")]
    prefix: String,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    let cli = Cli::parse();
    let token =
        std::env::var("DISCORD_TOKEN").expect("Missing `DISCORD_TOKEN` environment variable");

    let framework = poise::Framework::builder()
        .setup(move |ctx, _ready, _framework| {
            let ctx = ctx.clone();
            let db_path = cli.db.clone();
            Box::pin(async move {
                let db = tokio_rusqlite::Connection::open(&db_path).await?;
                db.call(|conn| Ok(db::init(conn)?)).await?;

                let quotes = db.call(|conn| Ok(db::all_quote_texts(conn)?)).await?;
                let model = markov::MarkovModel::build(markov::Order::default(), &quotes);

                let lol = Arc::new(lol::LolTracker::new());
                tokio::spawn(run_lol_tick(ctx, Arc::clone(&lol)));

                Ok(Data {
                    db,
                    markov: Arc::new(RwLock::new(model)),
                    lol,
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
                prefix: Some(cli.prefix.clone()),
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

    if new_message.content.to_lowercase().contains("my wife") {
        if let Some(guild_id) = new_message.guild_id {
            let murk = ctx
                .cache
                .guild(guild_id)
                .and_then(|guild| guild.emojis.values().find(|e| e.name == "murk").cloned());
            if let Some(murk) = murk {
                new_message.react(ctx, murk).await?;
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

async fn run_lol_tick(ctx: serenity::Context, lol: Arc<lol::LolTracker>) {
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

            match announcement
                .channel
                .say(&ctx.http, announcement.tier.message())
                .await
            {
                Ok(message) => {
                    lol.record_announcement(announcement.channel, announcement.tier, message.id)
                }
                Err(e) => tracing::error!("failed to send lol announcement: {e}"),
            }
        }
    }
}
