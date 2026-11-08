//! What a YAML text may grow to once its aliases are copied, held before
//! the document is built.
//!
//! `yaml-rust2` copies an anchored node whole at every alias, so ten
//! anchors each aliased ten times over the one before — billion laughs —
//! are a few hundred bytes of text and a billion nodes in memory. Aliases
//! are ordinary YAML: anchors and merge keys are how CI pipelines and
//! Kubernetes manifests share a block, so they are counted, not refused.
//! The parser's events are walked first, every anchored node weighed as its
//! nodes and scalar bytes, and every alias adds its anchor's weight to what
//! the copies come to; past [`ALIAS_BUDGET`], or nested past
//! [`codec::depth::MAX_DEPTH`] — an alias's copy counted as deep as what it
//! copies — the text is refused at the alias or the collection that crossed
//! it, and only a text within both is loaded.
//!
//! The walk pulls one event at a time rather than handing the parser a
//! receiver: `Parser::load`, which `YamlLoader` stands on, recurses once a
//! level, so ten thousand nested block sequences overflowed the stack
//! before any receiver could refuse them. Once the walk holds the text to
//! 64 levels, `YamlLoader` recurses no deeper than that.

use std::collections::HashMap;

use codec::depth;
use yaml_rust2::parser::{Event, Parser};
use yaml_rust2::scanner::Marker;
use yaml_rust2::{ScanError, Yaml, YamlLoader};

/// The most the copies aliases make may weigh in one text: a node counts
/// one, and a scalar its bytes besides. An honest document's shared blocks
/// come nowhere near it; billion laughs crosses it at its fifth level.
pub const ALIAS_BUDGET: u64 = 100_000;

/// Every document `text` holds, once its aliases and nesting are within
/// budget.
///
/// # Errors
/// The text is not YAML, or its aliases copy past [`ALIAS_BUDGET`], or it
/// nests past [`codec::depth::MAX_DEPTH`]: each where it stopped.
pub fn load(text: &str) -> Result<Vec<Yaml>, ScanError> {
    let mut parser = Parser::new_from_str(text);
    let mut budget = Budget::default();
    loop {
        let (event, mark) = parser.next_token()?;
        if event == Event::StreamEnd {
            break;
        }
        budget.event(event, mark)?;
    }
    YamlLoader::load_from_str(text)
}

/// One node as the budget sees it: what it weighs, and how many
/// collections deep it goes.
#[derive(Clone, Copy, Default)]
struct Size {
    weight: u64,
    height: usize,
}

/// The walk over the events: each anchor's size, the collections open and
/// the size of what is inside each so far, and what the copies come to.
#[derive(Default)]
struct Budget {
    anchors: HashMap<usize, Size>,
    open: Vec<(usize, Size)>,
    copied: u64,
}

impl Budget {
    fn event(&mut self, event: Event, mark: Marker) -> Result<(), ScanError> {
        let deep = |at: usize| {
            depth::within(at, "a YAML document")
                .map_err(|deep| ScanError::new_string(mark, deep.message))
        };
        match event {
            Event::SequenceStart(anchor, _) | Event::MappingStart(anchor, _) => {
                deep(self.open.len() + 1)?;
                self.open.push((anchor, Size::default()));
            }
            Event::SequenceEnd | Event::MappingEnd => {
                if let Some((anchor, inside)) = self.open.pop() {
                    let size = Size {
                        weight: inside.weight.saturating_add(1),
                        height: inside.height + 1,
                    };
                    self.node(anchor, size);
                }
            }
            Event::Scalar(text, _, anchor, _) => {
                let bytes = u64::try_from(text.len()).unwrap_or(u64::MAX);
                let size = Size {
                    weight: bytes.saturating_add(1),
                    height: 0,
                };
                self.node(anchor, size);
            }
            Event::Alias(anchor) => {
                let size = self.anchors.get(&anchor).copied().unwrap_or(Size {
                    weight: 1,
                    height: 0,
                });
                deep(self.open.len() + size.height)?;
                self.copied = self.copied.saturating_add(size.weight);
                if self.copied > ALIAS_BUDGET {
                    return Err(ScanError::new_string(
                        mark,
                        format!(
                            "its aliases copy past the {ALIAS_BUDGET} nodes and bytes Xmip builds"
                        ),
                    ));
                }
                self.node(0, size);
            }
            _ => {}
        }
        Ok(())
    }

    /// One finished node of `size`, remembered under `anchor` where it has
    /// one (yaml-rust2 numbers anchors from 1), and counted inside the
    /// collection it is in.
    fn node(&mut self, anchor: usize, size: Size) {
        if anchor > 0 {
            self.anchors.insert(anchor, size);
        }
        if let Some((_, inside)) = self.open.last_mut() {
            inside.weight = inside.weight.saturating_add(size.weight);
            inside.height = inside.height.max(size.height);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Billion laughs: each level aliases the one before ten times.
    fn laughs(levels: usize) -> String {
        let mut text = vec!["a0: &a0 \"lol\"".to_string()];
        for level in 1..levels {
            let previous = format!("*a{}", level - 1);
            let row = vec![previous; 10].join(", ");
            text.push(format!("a{level}: &a{level} [{row}]"));
        }
        text.join("\n")
    }

    #[test]
    fn billion_laughs_is_refused_at_the_alias_that_crosses_the_budget() {
        let refused = load(&laughs(10)).expect_err("a billion nodes");
        assert!(refused.info().contains("aliases copy past"), "{refused}");
        assert_eq!(refused.marker().line(), 6, "the fifth level crosses it");
        let shared = load(&laughs(3)).expect("a hundred copies");
        assert_eq!(shared.len(), 1);
    }

    #[test]
    fn the_contract_answers_billion_laughs_with_an_issue_not_a_crash() {
        use contract::Contract;
        let stream = contract::fixture::stream_as(&laughs(10), None);
        let verdict = crate::YamlContract::new()
            .validate(&stream)
            .expect("answers");
        assert!(!verdict.valid);
        assert!(verdict.issues[0].message.contains("aliases copy past"));
        assert!(crate::first_document(&stream).is_err());
    }

    #[test]
    fn an_honest_shared_block_and_its_merge_key_load() {
        let text = "base: &base\n  image: x\n  retries: 3\njob1:\n  <<: *base\n\
                    job2:\n  <<: *base\n  retries: 5\n";
        let documents = load(text).expect("loads");
        assert_eq!(documents[0]["job1"]["<<"]["image"].as_str(), Some("x"));
    }

    #[test]
    fn nesting_past_the_cap_is_refused_and_a_self_alias_is_not_a_loop() {
        let flow = format!("{}{}", "[".repeat(100_000), "]".repeat(100_000));
        assert!(load(&flow).is_err(), "the scanner's own limit");
        let block = format!("{}x", "- ".repeat(10_000));
        let refused = load(&block).expect_err("deep");
        assert!(
            refused.info().contains("nests past the 64 levels"),
            "{refused}"
        );
        let nested = format!("{}{}", "[".repeat(64), "]".repeat(64));
        assert!(load(&nested).is_ok());
        assert!(load("a: &a [*a, *a]").is_ok(), "an alias to an open anchor");
        let sixty = format!("{}{}", "[".repeat(60), "]".repeat(60));
        let chained = format!("a: &a {sixty}\nb: &b [[*a]]\nc: [[*b]]");
        let refused = load(&chained).expect_err("copies nest deeper");
        assert_eq!(refused.marker().line(), 3, "{refused}");
        assert!(load(&format!("a: &a {sixty}\nb: &b [[*a]]")).is_ok());
    }
}
