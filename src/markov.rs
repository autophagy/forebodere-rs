use markov_chain::Chain;

const FORBIDDEN_CHARACTERS: [char; 6] = ['(', ')', '"', '\'', '[', ']'];

fn tokenize(quote: &str) -> Vec<String> {
    quote
        .chars()
        .filter(|c| !FORBIDDEN_CHARACTERS.contains(c))
        .collect::<String>()
        .split_whitespace()
        .map(String::from)
        .collect()
}

pub const MIN_ORDER: u32 = 1;
pub const MAX_ORDER: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Order(u32);

impl Order {
    pub fn new(order: u32) -> Option<Self> {
        (MIN_ORDER..=MAX_ORDER)
            .contains(&order)
            .then_some(Order(order))
    }
}

impl Default for Order {
    fn default() -> Self {
        Order(2)
    }
}

pub struct MarkovModel {
    chain: Chain<String>,
}

impl MarkovModel {
    pub fn new(order: Order) -> Self {
        Self {
            chain: Chain::of_order(order.0 as usize),
        }
    }

    pub fn build<I, S>(order: Order, quotes: I) -> Self
    where
        I: IntoIterator<Item = S>,
        S: AsRef<str>,
    {
        let mut model = Self::new(order);
        for quote in quotes {
            model.feed(quote.as_ref());
        }
        model
    }

    pub fn feed(&mut self, quote: &str) {
        let tokens = tokenize(quote);
        if !tokens.is_empty() {
            self.chain.feed(&tokens);
        }
    }

    pub fn generate(&self) -> Option<String> {
        if self.chain.is_empty() {
            return None;
        }
        let tokens = self.chain.generate();
        (!tokens.is_empty()).then(|| tokens.join(" "))
    }
}

impl Default for MarkovModel {
    fn default() -> Self {
        Self::new(Order::default())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn order_rejects_out_of_range_values() {
        assert_eq!(Order::new(0), None);
        assert_eq!(Order::new(MAX_ORDER + 1), None);
        assert!(Order::new(MIN_ORDER).is_some());
        assert!(Order::new(MAX_ORDER).is_some());
    }

    #[test]
    fn empty_model_generates_none() {
        let model = MarkovModel::default();
        assert_eq!(model.generate(), None);
    }

    #[test]
    fn feeding_blank_text_leaves_the_model_empty() {
        let mut model = MarkovModel::default();
        model.feed("   ");
        model.feed("");
        assert_eq!(model.generate(), None);
    }

    #[test]
    fn forbidden_characters_are_stripped_before_feeding() {
        let mut model = MarkovModel::default();
        model.feed("(hello) \"world\" ['test']");
        assert_eq!(model.generate(), Some("hello world test".to_string()));
    }

    #[test]
    fn different_orders_can_be_built_from_the_same_corpus() {
        // No repeated words: guarantees one unambiguous path through the
        // chain regardless of order, so generation is deterministic.
        let quote = "quick brown foxes jump over lazy sleeping dogs";
        for raw_order in MIN_ORDER..=MAX_ORDER {
            let order = Order::new(raw_order).unwrap();
            let model = MarkovModel::build(order, [quote]);
            assert_eq!(model.generate(), Some(quote.to_string()));
        }
    }
}
