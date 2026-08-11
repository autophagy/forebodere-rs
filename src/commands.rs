use crate::{db, llm, markov, Context, Error};

const DISCORD_MESSAGE_LIMIT: usize = 2000;

/// Adds a quote to the database.
#[poise::command(prefix_command)]
pub async fn addquote(
    ctx: Context<'_>,
    #[rest]
    #[description = "The quote text"]
    text: Option<String>,
) -> Result<(), Error> {
    let Some(text) = text.map(|t| t.trim().to_string()).filter(|t| !t.is_empty()) else {
        ctx.say("No quote to add.").await?;
        return Ok(());
    };

    let submitter = ctx.author().tag();
    let submitted = db::SubmittedAt(now_unix());

    let db = ctx.data().db.clone();
    let quote_text = text.clone();
    let id = db
        .call(move |conn| {
            Ok(db::insert_quote(
                conn,
                &quote_text,
                Some(&submitter),
                submitted,
            )?)
        })
        .await?;

    ctx.data().markov.write().unwrap().feed(&text);

    ctx.say(format!("Added quote (id: {id})")).await?;
    Ok(())
}

/// Returns a quote. No argument: random. `id:N`: fetch by id. Otherwise:
/// search, returning a random match.
#[poise::command(prefix_command)]
pub async fn quote(
    ctx: Context<'_>,
    #[rest]
    #[description = "id:N, a search query, or nothing for a random quote"]
    arg: Option<String>,
) -> Result<(), Error> {
    let db = ctx.data().db.clone();
    let found = match parse_quote_request(arg.as_deref()) {
        QuoteRequest::Random => db.call(|conn| Ok(db::random_quote(conn)?)).await?,
        QuoteRequest::ById(id) => db.call(move |conn| Ok(db::quote_by_id(conn, id)?)).await?,
        QuoteRequest::Search(query) => {
            db.call(move |conn| Ok(db::search_random(conn, &query)?))
                .await?
        }
        QuoteRequest::InvalidId(text) => {
            ctx.say(format!("`{text}` isn't a valid quote id.")).await?;
            return Ok(());
        }
    };

    match found {
        Some(q) => ctx.say(format_quote(&q)).await?,
        None => ctx.say("No quote found.").await?,
    };
    Ok(())
}

/// Returns every quote matching a search.
#[poise::command(prefix_command)]
pub async fn quoteall(
    ctx: Context<'_>,
    #[rest]
    #[description = "Search query"]
    query: Option<String>,
) -> Result<(), Error> {
    let Some(query) = query
        .map(|q| q.trim().to_string())
        .filter(|q| !q.is_empty())
    else {
        ctx.say("Missing search string").await?;
        return Ok(());
    };

    let quotes = ctx
        .data()
        .db
        .call(move |conn| Ok(db::search_all(conn, &query)?))
        .await?;

    if quotes.is_empty() {
        ctx.say("No quotes found.").await?;
        return Ok(());
    }

    let lines: Vec<String> = quotes.iter().map(format_quote_line).collect();
    for chunk in chunk_messages(&lines, DISCORD_MESSAGE_LIMIT) {
        ctx.say(chunk).await?;
    }
    Ok(())
}

/// Generates a sentence from the quote corpus.
#[poise::command(prefix_command)]
pub async fn markov(
    ctx: Context<'_>,
    #[description = "Markov chain order (default: the bot's usual order)"] order: Option<u32>,
) -> Result<(), Error> {
    let sentence = match order {
        None => ctx.data().markov.read().unwrap().generate(),
        Some(raw_order) => {
            let Some(order) = markov::Order::new(raw_order) else {
                ctx.say(format!(
                    "Order must be between {} and {}.",
                    markov::MIN_ORDER,
                    markov::MAX_ORDER
                ))
                .await?;
                return Ok(());
            };
            let quotes = ctx
                .data()
                .db
                .call(|conn| Ok(db::all_quote_texts(conn)?))
                .await?;
            markov::MarkovModel::build(order, &quotes).generate()
        }
    };

    match sentence {
        Some(s) => ctx.say(s).await?,
        None => {
            ctx.say("Insufficient quote corpus to generate Markov chain.")
                .await?
        }
    };
    Ok(())
}

/// Hows it going
#[poise::command(prefix_command)]
pub async fn status(ctx: Context<'_>) -> Result<(), Error> {
    let count = ctx
        .data()
        .db
        .call(|conn| Ok(db::quote_count(conn)?))
        .await?;
    let queries = ctx
        .data()
        .queries
        .load(std::sync::atomic::Ordering::Relaxed);
    let uptime = ctx.data().started.elapsed();
    let latency = ctx.ping().await;

    let response = format!(
        "Bot Status:\n\
         ```\n\
         Quotes    :: {count}\n\
         Queries   :: {queries}\n\
         Uptime    :: {}\n\
         Latency   :: {}ms\n\
         Version   :: {}\n\
         ```\n\
         System Status:\n\
         ```\n\
         OS        :: {}\n\
         Arch      :: {}\n\
         ```",
        format_duration(uptime),
        latency.as_millis(),
        env!("CARGO_PKG_VERSION"),
        std::env::consts::OS,
        std::env::consts::ARCH,
    );
    ctx.say(response).await?;
    Ok(())
}

/// Trout-slaps a target, or yourself if no target is given.
#[poise::command(prefix_command)]
pub async fn slap(
    ctx: Context<'_>,
    #[rest]
    #[description = "Who to slap"]
    target: Option<String>,
) -> Result<(), Error> {
    let target = target
        .map(|t| t.trim().to_string())
        .filter(|t| !t.is_empty())
        .unwrap_or_else(|| ctx.author().name.clone());
    let bot_name = ctx.serenity_context().cache.current_user().name.clone();

    ctx.say(format!(
        "*{bot_name} slaps {target} around a bit with a large trout*"
    ))
    .await?;
    Ok(())
}

const GEN_PLACEHOLDER: &str = "\u{1F52E}\u{2728}...";
const GEN_TEMPERATURE: f32 = 0.8;
const GEN_MAX_TOKENS: u32 = 200;

/// Reach into the bowels of latent space and see what you find.
#[poise::command(prefix_command)]
pub async fn gen(
    ctx: Context<'_>,
    #[rest]
    #[description = "Optional seed prompt default"]
    prompt: Option<String>,
) -> Result<(), Error> {
    let Some(endpoint) = ctx.data().config.llm_endpoint.clone() else {
        ctx.say("No LLM endpoint configured (set `llm_endpoint` in the config).")
            .await?;
        return Ok(());
    };

    let instruction = prompt
        .as_deref()
        .map(str::trim)
        .filter(|p| !p.is_empty())
        .unwrap_or(llm::DEFAULT_INSTRUCTION);

    let placeholder = ctx.say(GEN_PLACEHOLDER).await?;

    let client = ctx.data().http.clone();
    let result = llm::generate_quote(
        &client,
        &endpoint,
        instruction,
        GEN_TEMPERATURE,
        GEN_MAX_TOKENS,
    )
    .await;

    let content = match result {
        Ok(text) if !text.is_empty() => text,
        Ok(_) => "Generated an empty response.".to_string(),
        Err(e) => {
            tracing::error!("gen failed: {e}");
            format!("Generation failed: {e}")
        }
    };

    placeholder
        .edit(ctx, poise::CreateReply::default().content(content))
        .await?;
    Ok(())
}

/// Lists available commands.
#[poise::command(prefix_command)]
pub async fn help(
    ctx: Context<'_>,
    #[description = "Specific command to show help about"] command: Option<String>,
) -> Result<(), Error> {
    poise::builtins::help(
        ctx,
        command.as_deref(),
        poise::builtins::HelpConfiguration::default(),
    )
    .await?;
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum QuoteRequest {
    Random,
    ById(db::QuoteId),
    InvalidId(String),
    Search(String),
}

fn parse_quote_request(arg: Option<&str>) -> QuoteRequest {
    let trimmed = arg.map(str::trim).filter(|s| !s.is_empty());
    match trimmed {
        None => QuoteRequest::Random,
        Some(text) => match text.strip_prefix("id:") {
            Some(rest) => match rest.trim().parse::<i64>() {
                Ok(id) => QuoteRequest::ById(db::QuoteId(id)),
                Err(_) => QuoteRequest::InvalidId(rest.trim().to_string()),
            },
            None => QuoteRequest::Search(text.to_string()),
        },
    }
}

fn sanitize_quote(quote: &str) -> String {
    let mut result = String::with_capacity(quote.len());
    let mut rest = quote;
    while let Some(start) = rest.find("<:") {
        result.push_str(&rest[..start]);
        let after_start = &rest[start..];
        match after_start.find('>') {
            Some(end) => {
                let token = &after_start[..=end];
                result.push_str(&token.replace('*', ""));
                rest = &after_start[end + 1..];
            }
            None => {
                result.push_str(after_start);
                rest = "";
                break;
            }
        }
    }
    result.push_str(rest);
    result
}

fn format_quote_line(quote: &db::Quote) -> String {
    format!("[{}] {}", quote.id, sanitize_quote(&quote.quote))
}

fn format_quote(quote: &db::Quote) -> String {
    let mut response = format_quote_line(quote);
    if let (Some(submitter), Some(submitted)) = (&quote.submitter, quote.submitted) {
        response.push_str(&format!(
            "\n\n*Submitted by {submitter} on {}*.",
            format_submitted(submitted)
        ));
    }
    response
}

fn format_submitted(submitted: db::SubmittedAt) -> String {
    match time::OffsetDateTime::from_unix_timestamp(submitted.0) {
        Ok(dt) => dt
            .format(&time::format_description::well_known::Rfc2822)
            .unwrap_or_else(|_| submitted.0.to_string()),
        Err(_) => submitted.0.to_string(),
    }
}

fn chunk_messages(lines: &[String], limit: usize) -> Vec<String> {
    let mut chunks: Vec<String> = Vec::new();
    for line in lines {
        match chunks.last_mut() {
            Some(chunk) if chunk.len() + 1 + line.len() <= limit => {
                chunk.push('\n');
                chunk.push_str(line);
            }
            _ => chunks.push(line.clone()),
        }
    }
    chunks
}

fn format_duration(duration: std::time::Duration) -> String {
    let total_secs = duration.as_secs();
    let days = total_secs / 86400;
    let hours = (total_secs % 86400) / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if days > 0 {
        format!("{days}d {hours}h {minutes}m {seconds}s")
    } else if hours > 0 {
        format!("{hours}h {minutes}m {seconds}s")
    } else if minutes > 0 {
        format!("{minutes}m {seconds}s")
    } else {
        format!("{seconds}s")
    }
}

fn now_unix() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs() as i64
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_quote_request_handles_empty_and_missing_args() {
        assert_eq!(parse_quote_request(None), QuoteRequest::Random);
        assert_eq!(parse_quote_request(Some("")), QuoteRequest::Random);
        assert_eq!(parse_quote_request(Some("   ")), QuoteRequest::Random);
    }

    #[test]
    fn parse_quote_request_parses_id_prefix() {
        assert_eq!(
            parse_quote_request(Some("id:2932")),
            QuoteRequest::ById(db::QuoteId(2932))
        );
        assert_eq!(
            parse_quote_request(Some("id: 2932 ")),
            QuoteRequest::ById(db::QuoteId(2932))
        );
    }

    #[test]
    fn parse_quote_request_rejects_non_numeric_id() {
        assert_eq!(
            parse_quote_request(Some("id:abc")),
            QuoteRequest::InvalidId("abc".to_string())
        );
    }

    #[test]
    fn parse_quote_request_falls_back_to_search() {
        assert_eq!(
            parse_quote_request(Some("quick fox")),
            QuoteRequest::Search("quick fox".to_string())
        );
    }

    #[test]
    fn sanitize_quote_strips_asterisks_only_inside_emoji_tokens() {
        assert_eq!(
            sanitize_quote("check out <:**custom**emoji:387878347> right"),
            "check out <:customemoji:387878347> right"
        );
        assert_eq!(sanitize_quote("the **quick** fox"), "the **quick** fox");
    }

    #[test]
    fn sanitize_quote_handles_multiple_tokens_and_plain_text() {
        assert_eq!(
            sanitize_quote("<:**a**:1> and <:**b**:2>"),
            "<:a:1> and <:b:2>"
        );
        assert_eq!(sanitize_quote("no emoji here"), "no emoji here");
        assert_eq!(sanitize_quote(""), "");
    }

    #[test]
    fn sanitize_quote_tolerates_an_unterminated_token() {
        assert_eq!(sanitize_quote("abc <:unterminated"), "abc <:unterminated");
    }

    #[test]
    fn chunk_messages_groups_lines_under_the_limit() {
        let lines = vec!["a".to_string(), "b".to_string(), "c".to_string()];
        assert_eq!(chunk_messages(&lines, 2000), vec!["a\nb\nc".to_string()]);
    }

    #[test]
    fn chunk_messages_keeps_an_oversized_line_whole() {
        let long = "x".repeat(50);
        let lines = vec![long.clone()];
        assert_eq!(chunk_messages(&lines, 10), vec![long]);
    }

    #[test]
    fn chunk_messages_handles_no_lines() {
        assert_eq!(chunk_messages(&[], 2000), Vec::<String>::new());
    }

    #[test]
    fn format_duration_formats_each_unit() {
        assert_eq!(format_duration(std::time::Duration::from_secs(5)), "5s");
        assert_eq!(format_duration(std::time::Duration::from_secs(65)), "1m 5s");
        assert_eq!(
            format_duration(std::time::Duration::from_secs(3661)),
            "1h 1m 1s"
        );
        assert_eq!(
            format_duration(std::time::Duration::from_secs(90061)),
            "1d 1h 1m 1s"
        );
    }

    #[test]
    fn format_submitted_renders_the_unix_epoch() {
        assert_eq!(
            format_submitted(db::SubmittedAt(0)),
            "Thu, 01 Jan 1970 00:00:00 +0000"
        );
    }
}
