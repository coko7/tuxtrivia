//! Print a round in the terminal: `cargo run --package tuxtrivia-core --example round [seed]`
//! Handy for eyeballing questions after editing data/people.toml.

use tuxtrivia_core::{Prompt, QuizConfig, bundled_roster, generate};

fn main() {
    let roster = bundled_roster().unwrap_or_else(|e| panic!("data/people.toml: {e}"));
    let seed = std::env::args()
        .nth(1)
        .and_then(|s| s.parse().ok())
        .unwrap_or(1);
    let mut rng = fastrand::Rng::with_seed(seed);

    for (n, q) in generate(&roster, &mut rng, &QuizConfig::default())
        .iter()
        .enumerate()
    {
        let subject = roster.person(q.subject);
        let text = match &q.prompt {
            Prompt::WhoCreated { creation } => format!("Who created {creation}?"),
            Prompt::WhatCreated => format!("What did {} create?", subject.name),
            Prompt::WhoIsThis => "Who is in this portrait?".to_string(),
            Prompt::GuessWho { clues } => format!("Guess who:\n     - {}", clues.join("\n     - ")),
            Prompt::WhenReleased { creation } => format!("When did {creation} first appear?"),
        };
        println!("{:>2}. {text}", n + 1);
        for (i, c) in q.choices.iter().enumerate() {
            let mark = if i == q.answer { "*" } else { " " };
            println!("   {mark} {}) {c}", i + 1);
        }
        println!();
    }
}
