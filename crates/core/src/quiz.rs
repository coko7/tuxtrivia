//! Turns the roster into rounds of multiple-choice questions.

use std::collections::HashSet;

use fastrand::Rng;

use crate::model::{Category, Creation, PersonIdx, Roster, key};

pub const CHOICES: usize = 4;
/// Upper bound for generated year distractors.
pub const LATEST_YEAR: i32 = 2026;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum QuestionKind {
    /// "Who created curl?" — pick a person.
    WhoCreated,
    /// "What did Daniel Stenberg create?" — pick a creation.
    WhatCreated,
    /// Portrait shown — pick a person. Needs a downloaded portrait.
    WhoIsThis,
    /// Clues revealed one by one — pick a person. Fewer clues, more points.
    GuessWho,
    /// "When was curl first released?" — pick a year.
    WhenReleased,
}

impl QuestionKind {
    pub const ALL: [QuestionKind; 5] = [
        QuestionKind::WhoCreated,
        QuestionKind::WhatCreated,
        QuestionKind::WhoIsThis,
        QuestionKind::GuessWho,
        QuestionKind::WhenReleased,
    ];
}

#[derive(Debug, Clone, PartialEq)]
pub enum Prompt {
    WhoCreated { creation: String },
    WhatCreated,
    WhoIsThis,
    GuessWho { clues: Vec<String> },
    WhenReleased { creation: String },
}

#[derive(Debug, Clone, PartialEq)]
pub struct Question {
    pub prompt: Prompt,
    /// The person the question is about; revealed after answering.
    pub subject: PersonIdx,
    pub choices: Vec<String>,
    /// Index of the correct entry in `choices`.
    pub answer: usize,
}

impl Question {
    pub fn kind(&self) -> QuestionKind {
        match self.prompt {
            Prompt::WhoCreated { .. } => QuestionKind::WhoCreated,
            Prompt::WhatCreated => QuestionKind::WhatCreated,
            Prompt::WhoIsThis => QuestionKind::WhoIsThis,
            Prompt::GuessWho { .. } => QuestionKind::GuessWho,
            Prompt::WhenReleased { .. } => QuestionKind::WhenReleased,
        }
    }

    /// Points for a correct answer. Guess-who pays more the fewer
    /// clues were needed.
    pub fn points(&self, clues_shown: usize) -> u32 {
        match &self.prompt {
            Prompt::GuessWho { clues } => {
                let extra = clues_shown.saturating_sub(1) as u32;
                (clues.len() as u32).saturating_sub(extra).max(1)
            }
            _ => 1,
        }
    }
}

#[derive(Debug, Clone)]
pub struct QuizConfig {
    pub categories: Vec<Category>,
    pub length: usize,
    pub kinds: Vec<QuestionKind>,
}

impl Default for QuizConfig {
    fn default() -> Self {
        QuizConfig {
            categories: Category::ALL.to_vec(),
            length: 10,
            kinds: QuestionKind::ALL.to_vec(),
        }
    }
}

/// Builds a round. Subjects are drawn without repetition from the chosen
/// categories (cycling if the round is longer than the pool). Distractors
/// come from the same category first, then from anyone.
pub fn generate(roster: &Roster, rng: &mut Rng, config: &QuizConfig) -> Vec<Question> {
    let mut pool: Vec<PersonIdx> = (0..roster.people().len())
        .filter(|&i| config.categories.contains(&roster.person(i).category))
        .collect();
    if pool.is_empty() || config.kinds.is_empty() {
        return Vec::new();
    }
    rng.shuffle(&mut pool);

    let mut questions = Vec::with_capacity(config.length);
    let mut cursor = 0;
    // Every person can produce WhoCreated/WhatCreated, so this terminates
    // quickly unless the config only allows kinds the data can't satisfy.
    let mut attempts = 0;
    while questions.len() < config.length && attempts < config.length * 20 {
        attempts += 1;
        let subject = pool[cursor % pool.len()];
        cursor += 1;

        let mut kinds = config.kinds.clone();
        rng.shuffle(&mut kinds);
        if let Some(q) = kinds.iter().find_map(|&k| build(roster, rng, k, subject)) {
            questions.push(q);
        }
    }
    questions
}

/// Builds one question of `kind` about `subject`, or `None` if the data
/// can't support it (no portrait, no dated creation, too few clues...).
pub fn build(
    roster: &Roster,
    rng: &mut Rng,
    kind: QuestionKind,
    subject: PersonIdx,
) -> Option<Question> {
    let person = roster.person(subject);
    match kind {
        QuestionKind::WhoCreated => {
            let creation = pick_fair_creation(roster, rng, subject)?;
            let exclude = roster
                .credited_with(&creation.name)
                .cloned()
                .unwrap_or_default();
            let (choices, answer) = person_choices(roster, rng, subject, &exclude)?;
            Some(Question {
                prompt: Prompt::WhoCreated {
                    creation: creation.name.clone(),
                },
                subject,
                choices,
                answer,
            })
        }
        QuestionKind::WhatCreated => {
            let correct = pick_fair_creation(roster, rng, subject)?;
            let own: HashSet<String> = person.creations.iter().map(|c| key(&c.name)).collect();
            let mut seen = own.clone();

            // Same-category creations first: "vi" vs "csh" is a fun trap,
            // "vi" vs "Haskell" is not.
            let mut same = Vec::new();
            let mut other = Vec::new();
            for (i, p) in roster.people().iter().enumerate() {
                if i == subject {
                    continue;
                }
                for c in &p.creations {
                    if own.contains(&key(&c.name)) || roster.is_eponymous(&c.name) {
                        continue;
                    }
                    if p.category == person.category {
                        same.push(&c.name)
                    } else {
                        other.push(&c.name)
                    }
                }
            }
            rng.shuffle(&mut same);
            rng.shuffle(&mut other);

            let mut choices = vec![correct.name.clone()];
            for name in same.into_iter().chain(other) {
                if choices.len() == CHOICES {
                    break;
                }
                if seen.insert(key(name)) {
                    choices.push(name.clone());
                }
            }
            let (choices, answer) = finish(rng, choices)?;
            Some(Question {
                prompt: Prompt::WhatCreated,
                subject,
                choices,
                answer,
            })
        }
        QuestionKind::WhoIsThis => {
            roster.portrait(subject)?;
            let (choices, answer) = person_choices(roster, rng, subject, &HashSet::new())?;
            Some(Question {
                prompt: Prompt::WhoIsThis,
                subject,
                choices,
                answer,
            })
        }
        QuestionKind::GuessWho => {
            if person.clues.len() < 2 {
                return None;
            }
            let (choices, answer) = person_choices(roster, rng, subject, &HashSet::new())?;
            Some(Question {
                prompt: Prompt::GuessWho {
                    clues: person.clues.clone(),
                },
                subject,
                choices,
                answer,
            })
        }
        QuestionKind::WhenReleased => {
            let dated: Vec<_> = person
                .creations
                .iter()
                .filter(|c| c.year.is_some())
                .collect();
            if dated.is_empty() {
                return None;
            }
            let creation = dated[rng.usize(..dated.len())];
            let year = creation.year?;

            let mut offsets = [-9, -6, -4, -3, -2, -1, 1, 2, 3, 4, 6, 9];
            rng.shuffle(&mut offsets);
            let mut years = vec![year];
            for off in offsets {
                if years.len() == CHOICES {
                    break;
                }
                let y = year + off;
                if y <= LATEST_YEAR {
                    years.push(y);
                }
            }
            years.sort_unstable();
            let answer = years.iter().position(|&y| y == year)?;
            Some(Question {
                prompt: Prompt::WhenReleased {
                    creation: creation.name.clone(),
                },
                subject,
                choices: years.iter().map(i32::to_string).collect(),
                answer,
            })
        }
    }
}

/// A random creation of `subject` whose name doesn't give the creator away.
fn pick_fair_creation<'r>(
    roster: &'r Roster,
    rng: &mut Rng,
    subject: PersonIdx,
) -> Option<&'r Creation> {
    let fair: Vec<_> = roster
        .person(subject)
        .creations
        .iter()
        .filter(|c| !roster.is_eponymous(&c.name))
        .collect();
    if fair.is_empty() {
        None
    } else {
        Some(fair[rng.usize(..fair.len())])
    }
}

/// Correct name plus three other people, preferring the same category.
fn person_choices(
    roster: &Roster,
    rng: &mut Rng,
    subject: PersonIdx,
    exclude: &HashSet<PersonIdx>,
) -> Option<(Vec<String>, usize)> {
    let category = roster.person(subject).category;
    let (mut same, mut other): (Vec<PersonIdx>, Vec<PersonIdx>) = (0..roster.people().len())
        .filter(|&i| i != subject && !exclude.contains(&i))
        .partition(|&i| roster.person(i).category == category);
    rng.shuffle(&mut same);
    rng.shuffle(&mut other);

    let mut choices = vec![roster.person(subject).name.clone()];
    choices.extend(
        same.into_iter()
            .chain(other)
            .take(CHOICES - 1)
            .map(|i| roster.person(i).name.clone()),
    );
    finish(rng, choices)
}

/// `choices[0]` is the correct answer; shuffle and report where it went.
fn finish(rng: &mut Rng, mut choices: Vec<String>) -> Option<(Vec<String>, usize)> {
    if choices.len() < CHOICES {
        return None;
    }
    let correct = choices[0].clone();
    rng.shuffle(&mut choices);
    let answer = choices.iter().position(|c| *c == correct)?;
    Some((choices, answer))
}
