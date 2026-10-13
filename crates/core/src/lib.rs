//! TuxTrivia core: the people roster and the question generator.
//! Kept free of any web dependency so it can be tested natively.

pub mod model;
pub mod quiz;

pub use model::{
    Category, Creation, Kind, LoadError, Person, PersonIdx, Portrait, PortraitsFile, Roster,
};
pub use quiz::{Prompt, Question, QuestionKind, QuizConfig, build, generate};

/// The bundled roster, as compiled into the game.
pub const PEOPLE_TOML: &str = include_str!("../../../data/people.toml");
pub const PORTRAITS_TOML: &str = include_str!("../../../data/portraits.toml");

pub fn bundled_roster() -> Result<Roster, LoadError> {
    Roster::from_toml(PEOPLE_TOML, PORTRAITS_TOML)
}

#[cfg(test)]
mod tests {
    use super::*;
    use fastrand::Rng;

    #[test]
    fn bundled_data_loads() {
        let roster = bundled_roster().expect("data/people.toml should be valid");
        assert!(roster.people().len() >= 50);
    }

    #[test]
    fn clues_never_name_the_person() {
        let roster = bundled_roster().unwrap();
        for p in roster.people() {
            let surname = model::surname(&p.name);
            for clue in &p.clues {
                let clue = clue.to_lowercase();
                assert!(
                    !clue.contains(&surname),
                    "clue for {} names them: {clue}",
                    p.id
                );
            }
        }
    }

    #[test]
    fn every_question_is_well_formed() {
        let roster = bundled_roster().unwrap();
        let mut rng = Rng::with_seed(42);
        for seed in 0..200 {
            rng.seed(seed);
            for q in generate(
                &roster,
                &mut rng,
                &QuizConfig {
                    length: 25,
                    ..Default::default()
                },
            ) {
                assert_eq!(q.choices.len(), quiz::CHOICES, "{q:?}");
                let unique: std::collections::HashSet<_> = q.choices.iter().collect();
                assert_eq!(unique.len(), quiz::CHOICES, "duplicate choice in {q:?}");
                assert!(q.answer < q.choices.len());
            }
        }
    }

    #[test]
    fn co_creators_are_never_distractors() {
        let roster = bundled_roster().unwrap();
        let mut rng = Rng::with_seed(7);
        for subject in 0..roster.people().len() {
            for _ in 0..20 {
                let Some(q) = build(&roster, &mut rng, QuestionKind::WhoCreated, subject) else {
                    continue;
                };
                let Prompt::WhoCreated { creation } = &q.prompt else {
                    unreachable!()
                };
                let credited = roster.credited_with(creation).unwrap();
                for (i, choice) in q.choices.iter().enumerate() {
                    if i == q.answer {
                        continue;
                    }
                    let idx = roster
                        .people()
                        .iter()
                        .position(|p| &p.name == choice)
                        .unwrap();
                    assert!(!credited.contains(&idx), "{choice} also made {creation}");
                }
            }
        }
    }

    #[test]
    fn what_created_distractors_are_not_the_subjects_work() {
        let roster = bundled_roster().unwrap();
        let mut rng = Rng::with_seed(3);
        for subject in 0..roster.people().len() {
            let Some(q) = build(&roster, &mut rng, QuestionKind::WhatCreated, subject) else {
                continue;
            };
            let own: Vec<_> = roster
                .person(subject)
                .creations
                .iter()
                .map(|c| c.name.to_lowercase())
                .collect();
            for (i, choice) in q.choices.iter().enumerate() {
                assert_eq!(i == q.answer, own.contains(&choice.to_lowercase()), "{q:?}");
            }
        }
    }

    #[test]
    fn creation_names_never_give_away_the_creator() {
        let roster = bundled_roster().unwrap();
        let mut rng = Rng::with_seed(11);
        for subject in 0..roster.people().len() {
            for kind in [QuestionKind::WhoCreated, QuestionKind::WhatCreated] {
                let Some(q) = build(&roster, &mut rng, kind, subject) else {
                    continue;
                };
                let shown = match &q.prompt {
                    Prompt::WhoCreated { creation } => vec![creation.clone()],
                    _ => q.choices.clone(),
                };
                for text in shown {
                    assert!(!roster.is_eponymous(&text), "{text} names its creator");
                }
            }
        }
    }

    #[test]
    fn everyone_can_appear_in_some_question() {
        let roster = bundled_roster().unwrap();
        let mut rng = Rng::with_seed(5);
        for subject in 0..roster.people().len() {
            let ok = QuestionKind::ALL
                .iter()
                .any(|&k| build(&roster, &mut rng, k, subject).is_some());
            assert!(ok, "{} can't be asked about", roster.person(subject).id);
        }
    }

    #[test]
    fn single_category_round_fills_up() {
        let roster = bundled_roster().unwrap();
        let mut rng = Rng::with_seed(1);
        for cat in Category::ALL {
            let config = QuizConfig {
                categories: vec![cat],
                length: 15,
                ..Default::default()
            };
            assert_eq!(generate(&roster, &mut rng, &config).len(), 15, "{cat:?}");
        }
    }

    #[test]
    fn guess_who_points_shrink_with_clues() {
        let q = Question {
            prompt: Prompt::GuessWho {
                clues: vec!["a".into(), "b".into(), "c".into()],
            },
            subject: 0,
            choices: vec![],
            answer: 0,
        };
        assert_eq!([q.points(1), q.points(2), q.points(3)], [3, 2, 1]);
    }
}
