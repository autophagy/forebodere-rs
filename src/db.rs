use rusqlite::{Connection, OptionalExtension, Result, Row};

const SCHEMA: &str = include_str!("../schema.sql");

pub fn init(conn: &Connection) -> Result<()> {
    let user_version: i64 = conn.query_row("PRAGMA user_version", [], |row| row.get(0))?;
    if user_version == 0 {
        conn.execute_batch(SCHEMA)?;
    }
    Ok(())
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct QuoteId(pub i64);

impl std::fmt::Display for QuoteId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}", self.0)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SubmittedAt(pub i64);

#[derive(Debug, Clone, PartialEq)]
pub struct Quote {
    pub id: QuoteId,
    pub quote: String,
    pub submitter: Option<String>,
    pub submitted: Option<SubmittedAt>,
}

fn row_to_quote(row: &Row) -> Result<Quote> {
    Ok(Quote {
        id: QuoteId(row.get(0)?),
        quote: row.get(1)?,
        submitter: row.get(2)?,
        submitted: row.get::<_, Option<i64>>(3)?.map(SubmittedAt),
    })
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum Query {
    /// The user wrapped their own query in quotes: an exact adjacent phrase.
    Phrase(String),
    /// Bare words: matched as an AND of literal terms, any order/position.
    Terms(Vec<String>),
}

impl Query {
    fn parse(input: &str) -> Option<Self> {
        let trimmed = input.trim();
        if trimmed.len() >= 2 && trimmed.starts_with('"') && trimmed.ends_with('"') {
            let inner = &trimmed[1..trimmed.len() - 1];
            return (!inner.is_empty()).then(|| Query::Phrase(inner.to_string()));
        }

        let terms: Vec<String> = trimmed.split_whitespace().map(str::to_string).collect();
        (!terms.is_empty()).then_some(Query::Terms(terms))
    }

    fn to_match_expr(&self) -> String {
        fn quote_literal(s: &str) -> String {
            format!("\"{}\"", s.replace('"', "\"\""))
        }

        match self {
            Query::Phrase(phrase) => quote_literal(phrase),
            Query::Terms(terms) => terms
                .iter()
                .map(|t| quote_literal(t))
                .collect::<Vec<_>>()
                .join(" "),
        }
    }
}

pub fn random_quote(conn: &Connection) -> Result<Option<Quote>> {
    conn.query_row(
        "SELECT id, quote, submitter, submitted FROM quotes ORDER BY RANDOM() LIMIT 1",
        [],
        row_to_quote,
    )
    .optional()
}

pub fn quote_by_id(conn: &Connection, id: QuoteId) -> Result<Option<Quote>> {
    conn.query_row(
        "SELECT id, quote, submitter, submitted FROM quotes WHERE id = ?1",
        [id.0],
        row_to_quote,
    )
    .optional()
}

pub fn search_random(conn: &Connection, query: &str) -> Result<Option<Quote>> {
    let Some(query) = Query::parse(query) else {
        return Ok(None);
    };
    conn.query_row(
        "SELECT q.id, highlight(quotes_fts, 0, '**', '**'), q.submitter, q.submitted
         FROM quotes_fts
         JOIN quotes q ON q.id = quotes_fts.rowid
         WHERE quotes_fts MATCH ?1
         ORDER BY RANDOM() LIMIT 1",
        [query.to_match_expr()],
        row_to_quote,
    )
    .optional()
}

pub fn search_all(conn: &Connection, query: &str) -> Result<Vec<Quote>> {
    let Some(query) = Query::parse(query) else {
        return Ok(vec![]);
    };
    let mut stmt = conn.prepare(
        "SELECT q.id, highlight(quotes_fts, 0, '**', '**'), q.submitter, q.submitted
         FROM quotes_fts
         JOIN quotes q ON q.id = quotes_fts.rowid
         WHERE quotes_fts MATCH ?1",
    )?;
    let rows = stmt.query_map([query.to_match_expr()], row_to_quote)?;
    rows.collect()
}

pub fn insert_quote(
    conn: &Connection,
    quote: &str,
    submitter: Option<&str>,
    submitted: SubmittedAt,
) -> Result<QuoteId> {
    conn.execute(
        "INSERT INTO quotes (quote, submitter, submitted) VALUES (?1, ?2, ?3)",
        (quote, submitter, submitted.0),
    )?;
    Ok(QuoteId(conn.last_insert_rowid()))
}

pub fn quote_count(conn: &Connection) -> Result<i64> {
    conn.query_row("SELECT COUNT(*) FROM quotes", [], |row| row.get(0))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_db() -> Connection {
        let conn = Connection::open_in_memory().unwrap();
        init(&conn).unwrap();
        conn
    }

    #[test]
    fn init_is_idempotent() {
        let conn = test_db();
        init(&conn).unwrap();
        assert_eq!(quote_count(&conn).unwrap(), 0);
    }

    #[test]
    fn random_quote_on_empty_db_is_none() {
        let conn = test_db();
        assert_eq!(random_quote(&conn).unwrap(), None);
    }

    #[test]
    fn insert_and_fetch_by_id() {
        let conn = test_db();
        let id = insert_quote(
            &conn,
            "hello world",
            Some("mika"),
            SubmittedAt(1_700_000_000),
        )
        .unwrap();

        let quote = quote_by_id(&conn, id).unwrap().unwrap();
        assert_eq!(quote.quote, "hello world");
        assert_eq!(quote.submitter.as_deref(), Some("mika"));
        assert_eq!(quote.submitted, Some(SubmittedAt(1_700_000_000)));

        assert_eq!(quote_by_id(&conn, QuoteId(id.0 + 1)).unwrap(), None);
    }

    #[test]
    fn insert_preserves_explicit_ids() {
        let conn = test_db();
        conn.execute(
            "INSERT INTO quotes (id, quote, submitter, submitted) VALUES (2932, 'old quote', NULL, NULL)",
            [],
        )
        .unwrap();

        let quote = quote_by_id(&conn, QuoteId(2932)).unwrap().unwrap();
        assert_eq!(quote.quote, "old quote");
        assert_eq!(quote.submitter, None);
        assert_eq!(quote.submitted, None);

        let next_id = insert_quote(&conn, "new quote", None, SubmittedAt(1)).unwrap();
        assert_eq!(next_id, QuoteId(2933));
    }

    #[test]
    fn random_quote_returns_a_row() {
        let conn = test_db();
        insert_quote(&conn, "only quote", None, SubmittedAt(1)).unwrap();
        let quote = random_quote(&conn).unwrap().unwrap();
        assert_eq!(quote.quote, "only quote");
    }

    #[test]
    fn search_random_bolds_matched_terms() {
        let conn = test_db();
        insert_quote(&conn, "the quick brown fox", None, SubmittedAt(1)).unwrap();
        insert_quote(&conn, "an unrelated quote", None, SubmittedAt(1)).unwrap();

        let quote = search_random(&conn, "quick").unwrap().unwrap();
        assert_eq!(quote.quote, "the **quick** brown fox");
    }

    #[test]
    fn search_random_bolds_every_occurrence_of_the_term() {
        let conn = test_db();
        insert_quote(
            &conn,
            "quick quick brown fox, so quick",
            None,
            SubmittedAt(1),
        )
        .unwrap();

        let quote = search_random(&conn, "quick").unwrap().unwrap();
        assert_eq!(quote.quote, "**quick** **quick** brown fox, so **quick**");
    }

    #[test]
    fn search_random_no_match_is_none() {
        let conn = test_db();
        insert_quote(&conn, "the quick brown fox", None, SubmittedAt(1)).unwrap();
        assert_eq!(search_random(&conn, "nonexistent").unwrap(), None);
    }

    #[test]
    fn search_all_returns_every_match() {
        let conn = test_db();
        insert_quote(&conn, "foo one", None, SubmittedAt(1)).unwrap();
        insert_quote(&conn, "foo two", None, SubmittedAt(1)).unwrap();
        insert_quote(&conn, "bar", None, SubmittedAt(1)).unwrap();

        let mut quotes = search_all(&conn, "foo").unwrap();
        quotes.sort_by_key(|q| q.id);
        assert_eq!(quotes.len(), 2);
        assert_eq!(quotes[0].quote, "**foo** one");
        assert_eq!(quotes[1].quote, "**foo** two");
    }

    #[test]
    fn quote_count_tracks_inserts() {
        let conn = test_db();
        assert_eq!(quote_count(&conn).unwrap(), 0);
        insert_quote(&conn, "a", None, SubmittedAt(1)).unwrap();
        insert_quote(&conn, "b", None, SubmittedAt(1)).unwrap();
        assert_eq!(quote_count(&conn).unwrap(), 2);
    }

    #[test]
    fn query_parse_handles_hostile_input() {
        assert_eq!(Query::parse("hello").unwrap().to_match_expr(), "\"hello\"");
        assert_eq!(
            Query::parse("quick fox").unwrap().to_match_expr(),
            "\"quick\" \"fox\""
        );
        assert_eq!(
            Query::parse("say \"hi\"").unwrap().to_match_expr(),
            "\"say\" \"\"\"hi\"\"\""
        );
        assert_eq!(
            Query::parse("\"quick fox\"").unwrap().to_match_expr(),
            "\"quick fox\""
        );
    }

    #[test]
    fn query_parse_rejects_empty_input() {
        assert_eq!(Query::parse(""), None);
        assert_eq!(Query::parse("   "), None);
        assert_eq!(
            Query::parse("\"\""),
            None,
            "an explicitly empty phrase is still empty"
        );
    }

    #[test]
    fn search_random_with_empty_query_is_none_without_hitting_fts5() {
        let conn = test_db();
        insert_quote(&conn, "the quick brown fox", None, SubmittedAt(1)).unwrap();
        assert_eq!(search_random(&conn, "   ").unwrap(), None);
    }

    #[test]
    fn search_all_with_empty_query_is_empty() {
        let conn = test_db();
        insert_quote(&conn, "the quick brown fox", None, SubmittedAt(1)).unwrap();
        assert_eq!(search_all(&conn, "").unwrap(), vec![]);
    }

    #[test]
    fn search_random_matches_unordered_terms() {
        let conn = test_db();
        insert_quote(&conn, "quick brown fox", None, SubmittedAt(1)).unwrap();

        let quote = search_random(&conn, "quick fox").unwrap().unwrap();
        assert_eq!(quote.quote, "**quick** brown **fox**");
    }

    #[test]
    fn search_random_explicit_phrase_requires_adjacency() {
        let conn = test_db();
        insert_quote(&conn, "quick brown fox", None, SubmittedAt(1)).unwrap();

        assert_eq!(
            search_random(&conn, "\"quick fox\"").unwrap(),
            None,
            "quick and fox aren't adjacent, so an explicit phrase query shouldn't match"
        );
        assert!(search_random(&conn, "\"quick brown\"").unwrap().is_some());
    }

    #[test]
    fn search_tolerates_fts5_operator_syntax() {
        let conn = test_db();
        insert_quote(
            &conn,
            "NEAR misses \"quoted\" text * and stuff",
            None,
            SubmittedAt(1),
        )
        .unwrap();

        // These would be syntax errors as raw FTS5 MATCH expressions; escaping
        // must make them safe literal phrase searches instead.
        for hostile in ["NEAR misses", "\"quoted\"", "text *", "unbalanced \""] {
            search_all(&conn, hostile).unwrap();
        }
    }
}
