use dashmap::DashMap;
use poise::serenity_prelude as serenity;
use serenity::{ChannelId, MessageId, UserId};
use std::time::{Duration, Instant};

const QUIET_GAP: Duration = Duration::from_secs(5);
const LAUGH_ROOTS: [&str; 5] = ["lol", "lo", "lmao", "rofl", "ha"];

fn normalize(content: &str) -> String {
    content
        .chars()
        .filter(|c| c.is_alphanumeric())
        .flat_map(char::to_lowercase)
        .collect()
}

fn is_repetition(normalized: &str, root: &str) -> bool {
    !normalized.is_empty()
        && normalized.len() >= root.len()
        && normalized
            .bytes()
            .enumerate()
            .all(|(i, b)| b == root.as_bytes()[i % root.len()])
}

fn is_laugh(content: &str) -> bool {
    let normalized = normalize(content);
    LAUGH_ROOTS
        .iter()
        .any(|root| is_repetition(&normalized, root))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum Tier {
    Multilol,
    Ultralol,
    Monsterlol,
}

impl Tier {
    fn from_frequency(frequency: u32) -> Option<Self> {
        match frequency {
            0..=2 => None,
            3 => Some(Tier::Multilol),
            4 => Some(Tier::Ultralol),
            _ => Some(Tier::Monsterlol),
        }
    }

    pub fn message(&self) -> &'static str {
        match self {
            Tier::Multilol => "Multilol!",
            Tier::Ultralol => "Ultralol!",
            Tier::Monsterlol => "M-M-M-MONSTERLOL!",
        }
    }
}

#[derive(Default)]
struct ChannelState {
    frequency: u32,
    last_author: Option<UserId>,
    last_laugh_at: Option<Instant>,
    announced_tier: Option<Tier>,
    announcement_message_id: Option<MessageId>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Announcement {
    pub channel: ChannelId,
    pub tier: Tier,
    pub previous_message_id: Option<MessageId>,
}

#[derive(Default)]
pub struct LolTracker {
    channels: DashMap<ChannelId, ChannelState>,
}

impl LolTracker {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn handle(&self, channel: ChannelId, author: UserId, content: &str, now: Instant) {
        let mut state = self.channels.entry(channel).or_default();

        if is_laugh(content) {
            if state.last_author != Some(author) {
                state.frequency += 1;
                state.last_author = Some(author);
                state.last_laugh_at = Some(now);
            }
        } else {
            state.frequency = 0;
            state.last_author = None;
            state.announced_tier = None;
            state.announcement_message_id = None;
        }
    }

    pub fn due_announcements(&self, now: Instant) -> Vec<Announcement> {
        self.channels
            .iter()
            .filter_map(|entry| {
                let state = entry.value();
                let tier = Tier::from_frequency(state.frequency)?;
                let elapsed = now.checked_duration_since(state.last_laugh_at?)?;
                if elapsed < QUIET_GAP {
                    return None;
                }
                if state
                    .announced_tier
                    .is_some_and(|announced| announced >= tier)
                {
                    return None;
                }
                Some(Announcement {
                    channel: *entry.key(),
                    tier,
                    previous_message_id: state.announcement_message_id,
                })
            })
            .collect()
    }

    pub fn record_announcement(&self, channel: ChannelId, tier: Tier, message_id: MessageId) {
        if let Some(mut state) = self.channels.get_mut(&channel) {
            state.announced_tier = Some(tier);
            state.announcement_message_id = Some(message_id);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn uid(n: u64) -> UserId {
        UserId::new(n)
    }

    fn cid(n: u64) -> ChannelId {
        ChannelId::new(n)
    }

    fn mid(n: u64) -> MessageId {
        MessageId::new(n)
    }

    #[test]
    fn is_laugh_recognises_the_base_words_case_insensitively() {
        for word in ["lol", "LOL", "LoL", "lmao", "rofl", "haha"] {
            assert!(is_laugh(word), "{word} should be a laugh");
        }
    }

    #[test]
    fn is_laugh_tolerates_surrounding_punctuation_and_whitespace() {
        for word in [" lol", "lol ", "lol!", "lol...", "lol?!", "  haha!  "] {
            assert!(is_laugh(word), "{word:?} should be a laugh");
        }
    }

    #[test]
    fn is_laugh_allows_repeated_roots() {
        for word in [
            "lollol",
            "hahaha",
            "hahahaha",
            "lmaolmao",
            "lolol",
            "lolololol",
        ] {
            assert!(is_laugh(word), "{word} should be a laugh");
        }
    }

    #[test]
    fn distinct_authors_build_a_streak() {
        let tracker = LolTracker::new();
        let now = Instant::now();
        let channel = cid(1);

        tracker.handle(channel, uid(1), "lol", now);
        tracker.handle(channel, uid(2), "lol", now);
        tracker.handle(channel, uid(3), "lol", now);

        // Not due yet: no quiet gap has elapsed.
        assert_eq!(tracker.due_announcements(now), vec![]);

        let after_gap = now + QUIET_GAP;
        assert_eq!(
            tracker.due_announcements(after_gap),
            vec![Announcement {
                channel,
                tier: Tier::Multilol,
                previous_message_id: None,
            }]
        );
    }

    #[test]
    fn same_author_repeating_is_frozen() {
        let tracker = LolTracker::new();
        let now = Instant::now();
        let channel = cid(1);

        tracker.handle(channel, uid(1), "lol", now);
        // Same author again, later: should not advance the streak, and
        // should not even bump the quiet-gap clock.
        tracker.handle(channel, uid(1), "lol", now + Duration::from_secs(1));
        tracker.handle(channel, uid(1), "lol", now + Duration::from_secs(2));

        // Still only frequency 1 - never reaches the tier-3 threshold.
        assert_eq!(
            tracker.due_announcements(now + Duration::from_secs(100)),
            vec![]
        );
    }

    #[test]
    fn non_laugh_message_resets_the_streak() {
        let tracker = LolTracker::new();
        let now = Instant::now();
        let channel = cid(1);

        tracker.handle(channel, uid(1), "lol", now);
        tracker.handle(channel, uid(2), "lol", now);
        tracker.handle(channel, uid(3), "lol", now);
        tracker.handle(channel, uid(4), "actually that's not funny", now);

        assert_eq!(tracker.due_announcements(now + QUIET_GAP), vec![]);
    }

    #[test]
    fn frequency_below_three_never_fires() {
        let tracker = LolTracker::new();
        let now = Instant::now();
        let channel = cid(1);

        tracker.handle(channel, uid(1), "lol", now);
        tracker.handle(channel, uid(2), "lol", now);

        assert_eq!(
            tracker.due_announcements(now + Duration::from_secs(1000)),
            vec![]
        );
    }

    #[test]
    fn escalation_reports_the_previous_message_for_deletion() {
        let tracker = LolTracker::new();
        let now = Instant::now();
        let channel = cid(1);

        tracker.handle(channel, uid(1), "lol", now);
        tracker.handle(channel, uid(2), "lol", now);
        tracker.handle(channel, uid(3), "lol", now);

        let after_first_gap = now + QUIET_GAP;
        let due = tracker.due_announcements(after_first_gap);
        assert_eq!(
            due,
            vec![Announcement {
                channel,
                tier: Tier::Multilol,
                previous_message_id: None,
            }]
        );
        tracker.record_announcement(channel, Tier::Multilol, mid(101));

        let fourth_laugh_at = after_first_gap + Duration::from_secs(1);
        tracker.handle(channel, uid(4), "lol", fourth_laugh_at);

        let after_second_gap = fourth_laugh_at + QUIET_GAP;
        assert_eq!(
            tracker.due_announcements(after_second_gap),
            vec![Announcement {
                channel,
                tier: Tier::Ultralol,
                previous_message_id: Some(mid(101)),
            }]
        );
    }

    #[test]
    fn channels_are_tracked_independently() {
        let tracker = LolTracker::new();
        let now = Instant::now();

        tracker.handle(cid(1), uid(1), "lol", now);
        tracker.handle(cid(1), uid(2), "lol", now);
        tracker.handle(cid(1), uid(3), "lol", now);

        tracker.handle(cid(2), uid(1), "lol", now);

        let due = tracker.due_announcements(now + QUIET_GAP);
        assert_eq!(
            due,
            vec![Announcement {
                channel: cid(1),
                tier: Tier::Multilol,
                previous_message_id: None,
            }]
        );
    }
}
