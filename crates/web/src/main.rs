//! TuxTrivia browser frontend.
//!
//! All game logic lives in `tuxtrivia-core`; this file is screens, state
//! and keyboard handling. The roster is compiled in, so the whole game is
//! a static bundle with no backend.

use std::sync::LazyLock;

use leptos::{ev, prelude::*};
use tuxtrivia_core::{
    Category, PersonIdx, Prompt, Question, QuestionKind, QuizConfig, Roster, bundled_roster,
    generate,
};

static ROSTER: LazyLock<Roster> =
    LazyLock::new(|| bundled_roster().expect("bundled data is validated by `cargo test`"));

const LENGTHS: [usize; 3] = [10, 20, 30];
const BEST_STREAK_KEY: &str = "tuxtrivia.best_streak";

fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(App);
}

// ─── State ──────────────────────────────────────────────────────────────

#[derive(Clone, Copy, PartialEq, Eq)]
enum Screen {
    Menu,
    Playing,
    Results,
    Credits,
}

#[derive(Clone, Default)]
struct Game {
    questions: Vec<Question>,
    idx: usize,
    score: u32,
    correct: u32,
    streak: u32,
    best_streak: u32,
    picked: Option<usize>,
    clues_shown: usize,
    missed: Vec<PersonIdx>,
}

impl Game {
    fn new(questions: Vec<Question>) -> Self {
        Game {
            questions,
            clues_shown: 1,
            ..Default::default()
        }
    }

    fn current(&self) -> &Question {
        &self.questions[self.idx.min(self.questions.len() - 1)]
    }

    fn pick(&mut self, choice: usize) {
        if self.picked.is_some() || choice >= self.current().choices.len() {
            return;
        }
        self.picked = Some(choice);
        let q = self.current().clone();
        if choice == q.answer {
            self.score += q.points(self.clues_shown);
            self.correct += 1;
            self.streak += 1;
            self.best_streak = self.best_streak.max(self.streak);
        } else {
            self.streak = 0;
            if !self.missed.contains(&q.subject) {
                self.missed.push(q.subject);
            }
        }
    }

    fn reveal_clue(&mut self) {
        if self.picked.is_some() {
            return;
        }
        if let Prompt::GuessWho { clues } = &self.current().prompt
            && self.clues_shown < clues.len()
        {
            self.clues_shown += 1;
        }
    }

    /// Moves to the next question. Returns true when the round is over.
    fn advance(&mut self) -> bool {
        if self.picked.is_none() {
            return false;
        }
        self.idx += 1;
        self.picked = None;
        self.clues_shown = 1;
        self.idx >= self.questions.len()
    }

    fn max_score(&self) -> u32 {
        self.questions.iter().map(|q| q.points(1)).sum()
    }
}

// ─── Browser helpers ────────────────────────────────────────────────────

fn rng() -> fastrand::Rng {
    let random = (js_sys::Math::random() * (1u64 << 53) as f64) as u64;
    let now = js_sys::Date::now() as u64;
    fastrand::Rng::with_seed(random ^ now.rotate_left(32))
}

fn storage() -> Option<web_sys::Storage> {
    web_sys::window()?.local_storage().ok()?
}

fn load_u32(key: &str) -> u32 {
    storage()
        .and_then(|s| s.get_item(key).ok().flatten())
        .and_then(|v| v.parse().ok())
        .unwrap_or(0)
}

fn save_u32(key: &str, value: u32) {
    if let Some(s) = storage() {
        let _ = s.set_item(key, &value.to_string());
    }
}

fn best_score_key(length: usize) -> String {
    format!("tuxtrivia.best_score.{length}")
}

fn wikipedia_url(title: &str) -> String {
    format!("https://en.wikipedia.org/wiki/{title}")
}

// ─── App ────────────────────────────────────────────────────────────────

#[component]
fn App() -> impl IntoView {
    let screen = RwSignal::new(Screen::Menu);
    let game = RwSignal::new(Game::default());
    let categories = RwSignal::new(Category::ALL.to_vec());
    let length = RwSignal::new(LENGTHS[0]);
    let best_streak = RwSignal::new(load_u32(BEST_STREAK_KEY));

    let start = move || {
        let config = QuizConfig {
            categories: categories.get_untracked(),
            length: length.get_untracked(),
            kinds: QuestionKind::ALL.to_vec(),
        };
        let questions = generate(&ROSTER, &mut rng(), &config);
        if questions.is_empty() {
            return;
        }
        game.set(Game::new(questions));
        screen.set(Screen::Playing);
    };

    let next = move || {
        if game.try_update(Game::advance).unwrap_or(false) {
            let g = game.get_untracked();
            if g.best_streak > best_streak.get_untracked() {
                best_streak.set(g.best_streak);
                save_u32(BEST_STREAK_KEY, g.best_streak);
            }
            let key = best_score_key(g.questions.len());
            if g.score > load_u32(&key) {
                save_u32(&key, g.score);
            }
            screen.set(Screen::Results);
        }
    };

    // Keyboard: 1–4 answer, c for another clue, Enter for next / start.
    window_event_listener(ev::keydown, move |e: web_sys::KeyboardEvent| {
        if e.ctrl_key() || e.meta_key() || e.alt_key() || e.repeat() {
            return;
        }
        let key = e.key();
        let handled = match (screen.get_untracked(), key.as_str()) {
            (Screen::Playing, "1" | "2" | "3" | "4") => {
                let i = key.parse::<usize>().unwrap_or(1) - 1;
                game.update(|g| g.pick(i));
                true
            }
            (Screen::Playing, "c" | "C") => {
                game.update(Game::reveal_clue);
                true
            }
            (Screen::Playing, "Enter" | "n" | "N") => {
                next();
                true
            }
            (Screen::Menu | Screen::Results, "Enter") => {
                start();
                true
            }
            _ => false,
        };
        if handled {
            e.prevent_default();
        }
    });

    view! {
        <header class="topbar">
            <button class="brand" on:click=move |_| screen.set(Screen::Menu)>
                <span class="prompt">"$"</span>" tuxtrivia"<span class="cursor">"_"</span>
            </button>
            <nav>
                <button class="link" on:click=move |_| screen.set(Screen::Credits)>"credits"</button>
            </nav>
        </header>
        <main>
            {move || match screen.get() {
                Screen::Menu => menu_view(categories, length, best_streak, start).into_any(),
                Screen::Playing => play_view(game, next).into_any(),
                Screen::Results => results_view(game, best_streak, start, screen).into_any(),
                Screen::Credits => credits_view(screen).into_any(),
            }}
        </main>
    }
}

// ─── Screens ────────────────────────────────────────────────────────────

fn menu_view(
    categories: RwSignal<Vec<Category>>,
    length: RwSignal<usize>,
    best_streak: RwSignal<u32>,
    start: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    let decks = Category::ALL
        .into_iter()
        .map(|cat| {
            let count = ROSTER.count_in(&[cat]);
            let toggle = move |_| {
                categories.update(|c| match c.iter().position(|x| *x == cat) {
                    Some(p) => {
                        c.remove(p);
                    }
                    None => c.push(cat),
                })
            };
            view! {
                <label class="deck">
                    <input
                        type="checkbox"
                        prop:checked=move || categories.with(|c| c.contains(&cat))
                        on:change=toggle
                    />
                    <span class="deck-name">{cat.label()}</span>
                    <span class="deck-count">{count}</span>
                </label>
            }
        })
        .collect_view();

    let lengths = LENGTHS
        .into_iter()
        .map(|n| {
            view! {
                <button
                    class="chip"
                    class:active=move || length.get() == n
                    on:click=move |_| length.set(n)
                >
                    {n}
                </button>
            }
        })
        .collect_view();

    let selected = move || categories.with(|c| ROSTER.count_in(c));

    view! {
        <section class="card menu">
            <h1>"How well do you know the people who built your stack?"</h1>
            <p class="muted">
                {ROSTER.people().len()}
                " computer scientists, from the Analytical Engine to your Wayland compositor."
            </p>

            <h2 class="label">"Decks"</h2>
            <div class="decks">{decks}</div>

            <h2 class="label">"Questions"</h2>
            <div class="chips">{lengths}</div>

            <button
                class="primary"
                disabled=move || selected() == 0
                on:click=move |_| start()
            >
                "Start round ▸"
            </button>

            <p class="hint">
                <kbd>"1"</kbd>"–"<kbd>"4"</kbd>" answer · "<kbd>"c"</kbd>" another clue · "
                <kbd>"Enter"</kbd>" next"
            </p>
            {move || {
                let best = best_streak.get();
                (best > 0).then(|| view! { <p class="muted small">"Best streak: " {best}</p> })
            }}
        </section>
    }
}

fn play_view(
    game: RwSignal<Game>,
    next: impl Fn() + Copy + Send + Sync + 'static,
) -> impl IntoView {
    move || {
        let g = game.get();
        if g.questions.is_empty() {
            return view! { <p>"No questions."</p> }.into_any();
        }
        let q = g.current().clone();
        let subject = ROSTER.person(q.subject);
        let picked = g.picked;

        let prompt = match &q.prompt {
            Prompt::WhoCreated { creation } => view! {
                <h1 class="question">"Who created " <em>{creation.clone()}</em> "?"</h1>
            }
            .into_any(),
            Prompt::WhatCreated => view! {
                <div class="with-portrait">
                    {portrait_view(q.subject, false)}
                    <h1 class="question">"What did " <em>{subject.name.clone()}</em> " create?"</h1>
                </div>
            }
            .into_any(),
            Prompt::WhoIsThis => view! {
                <div class="who-is-this">
                    {portrait_view(q.subject, true)}
                    <h1 class="question">"Who is this?"</h1>
                </div>
            }
            .into_any(),
            Prompt::GuessWho { clues } => {
                let shown = clues[..g.clues_shown].to_vec();
                let more = picked.is_none() && g.clues_shown < clues.len();
                let worth = q.points(g.clues_shown);
                view! {
                    <h1 class="question">"Guess who"</h1>
                    <ol class="clues">
                        {shown.into_iter().map(|c| view! { <li>{c}</li> }).collect_view()}
                    </ol>
                    <div class="clue-bar">
                        <span class="muted small">"Worth " {worth} " pt" {(worth > 1).then_some("s")}</span>
                        {more.then(|| view! {
                            <button class="link" on:click=move |_| game.update(Game::reveal_clue)>
                                "another clue " <kbd>"c"</kbd>
                            </button>
                        })}
                    </div>
                }
                .into_any()
            }
            Prompt::WhenReleased { creation } => view! {
                <h1 class="question">"When did " <em>{creation.clone()}</em> " first appear?"</h1>
            }
            .into_any(),
        };

        let choices = q
            .choices
            .iter()
            .enumerate()
            .map(|(i, label)| {
                let class = match picked {
                    None => "choice",
                    Some(_) if i == q.answer => "choice correct",
                    Some(p) if p == i => "choice wrong",
                    Some(_) => "choice dim",
                };
                view! {
                    <li>
                        <button class=class disabled=picked.is_some() on:click=move |_| game.update(|g| g.pick(i))>
                            <kbd>{i + 1}</kbd>
                            <span>{label.clone()}</span>
                        </button>
                    </li>
                }
            })
            .collect_view();

        let last = g.idx + 1 == g.questions.len();
        let reveal = picked.map(|p| {
            let right = p == q.answer;
            view! {
                <div class="reveal" class:right=right class:missed=!right>
                    <p class="verdict">{if right { "Correct!" } else { "Not quite." }}</p>
                    {person_card(q.subject)}
                    <button class="primary" on:click=move |_| next()>
                        {if last { "See results ▸" } else { "Next ▸" }}
                    </button>
                </div>
            }
        });

        view! {
            <section class="card play">
                <div class="status">
                    <span>{g.idx + 1} " / " {g.questions.len()}</span>
                    <span>"score " <strong>{g.score}</strong></span>
                    <span class:hot={g.streak >= 3}>"streak " {g.streak}</span>
                </div>
                <progress max=g.questions.len() value=g.idx + usize::from(picked.is_some())></progress>
                {prompt}
                <ul class="choices">{choices}</ul>
                {reveal}
            </section>
        }
        .into_any()
    }
}

fn results_view(
    game: RwSignal<Game>,
    best_streak: RwSignal<u32>,
    start: impl Fn() + Copy + Send + Sync + 'static,
    screen: RwSignal<Screen>,
) -> impl IntoView {
    let g = game.get_untracked();
    let total = g.questions.len();
    let best_score = load_u32(&best_score_key(total));
    let headline = match g.correct as usize * 100 / total.max(1) {
        100 => "Flawless. You could teach this.",
        80.. => "Excellent — you know your history.",
        50.. => "Solid. A few names to look up.",
        _ => "Plenty of great people to discover.",
    };

    let missed = g
        .missed
        .iter()
        .map(|&idx| {
            let p = ROSTER.person(idx);
            let made = p
                .creations
                .iter()
                .map(|c| c.name.as_str())
                .collect::<Vec<_>>()
                .join(", ");
            let name = match &p.wikipedia {
                Some(title) => view! {
                    <a href=wikipedia_url(title) target="_blank" rel="noopener">{p.name.clone()}</a>
                }
                .into_any(),
                None => view! { <span>{p.name.clone()}</span> }.into_any(),
            };
            view! { <li>{name} <span class="muted">" — " {made}</span></li> }
        })
        .collect_view();

    view! {
        <section class="card results">
            <h1>{headline}</h1>
            <div class="stats">
                <div><strong>{g.score}</strong><span>"points of " {g.max_score()}</span></div>
                <div><strong>{g.correct}</strong><span>"correct of " {total}</span></div>
                <div><strong>{g.best_streak}</strong><span>"best streak"</span></div>
            </div>
            <p class="muted small">
                "Best for " {total} "-question rounds: " {best_score}
                " · all-time best streak: " {move || best_streak.get()}
            </p>
            {(!g.missed.is_empty()).then(|| view! {
                <h2 class="label">"Worth reading about"</h2>
                <ul class="missed">{missed}</ul>
            })}
            <div class="actions">
                <button class="primary" on:click=move |_| start()>"Play again ▸"</button>
                <button class="secondary" on:click=move |_| screen.set(Screen::Menu)>"Change decks"</button>
            </div>
        </section>
    }
}

fn credits_view(screen: RwSignal<Screen>) -> impl IntoView {
    let portraits = ROSTER
        .portraits()
        .map(|(person, portrait)| {
            view! {
                <li>
                    <strong>{person.name.clone()}</strong>
                    " — " {portrait.artist.clone()} ", "
                    {license_link(portrait.license.clone(), portrait.license_url.clone())}
                    ", via "
                    <a href=portrait.source_url.clone() target="_blank" rel="noopener">"Wikimedia Commons"</a>
                </li>
            }
        })
        .collect_view();
    let has_portraits = ROSTER.portraits().next().is_some();

    view! {
        <section class="card credits">
            <h1>"Credits"</h1>
            <p>
                "TuxTrivia is a small game about the people behind the tools we use every day. "
                "Facts are hand-written; if you spot a mistake, it's a bug."
            </p>
            <h2 class="label">"Portraits"</h2>
            {if has_portraits {
                view! { <ul class="credit-list">{portraits}</ul> }.into_any()
            } else {
                view! {
                    <p class="muted">
                        "No portraits yet. Run " <code>"cargo run --package tuxtrivia-portraits"</code>
                        " to fetch freely licensed ones from Wikimedia Commons."
                    </p>
                }
                .into_any()
            }}
            <button class="secondary" on:click=move |_| screen.set(Screen::Menu)>"◂ Back"</button>
        </section>
    }
}

// ─── Pieces ─────────────────────────────────────────────────────────────

fn license_link(license: String, url: Option<String>) -> AnyView {
    match url {
        Some(url) => view! { <a href=url target="_blank" rel="noopener">{license}</a> }.into_any(),
        None => view! { <span>{license}</span> }.into_any(),
    }
}

/// Portrait with its attribution, or an initials placeholder.
fn portrait_view(idx: PersonIdx, large: bool) -> AnyView {
    let person = ROSTER.person(idx);
    match ROSTER.portrait(idx) {
        Some(p) => view! {
            <figure class="portrait" class:large=large>
                <img src=format!("portraits/{}", p.file) alt="Portrait" loading="lazy" />
                <figcaption>
                    {p.artist.clone()} " · "
                    {license_link(p.license.clone(), p.license_url.clone())} " · "
                    <a href=p.source_url.clone() target="_blank" rel="noopener">"source"</a>
                </figcaption>
            </figure>
        }
        .into_any(),
        None => {
            let initials: String = person
                .name
                .split_whitespace()
                .filter(|w| w.chars().next().is_some_and(char::is_uppercase))
                .filter_map(|w| w.chars().next())
                .take(2)
                .collect();
            view! { <div class="portrait placeholder" class:large=large aria-hidden="true">{initials}</div> }
                .into_any()
        }
    }
}

/// Shown after answering: who it was and what they made.
fn person_card(idx: PersonIdx) -> impl IntoView {
    let p = ROSTER.person(idx);
    let meta = [p.born.map(|y| format!("born {y}")), p.country.clone()]
        .into_iter()
        .flatten()
        .collect::<Vec<_>>()
        .join(" · ");
    let creations = p
        .creations
        .iter()
        .map(|c| {
            let year = c.year.map(|y| format!(" ({y})")).unwrap_or_default();
            view! { <li>{c.name.clone()} <span class="muted">{year}</span></li> }
        })
        .collect_view();
    let link = p.wikipedia.as_ref().map(|t| {
        view! { <a class="small" href=wikipedia_url(t) target="_blank" rel="noopener">"Read more on Wikipedia ↗"</a> }
    });

    view! {
        <div class="person">
            {portrait_view(idx, false)}
            <div>
                <h3>{p.name.clone()}</h3>
                <p class="muted small">{meta}</p>
                <ul class="creations">{creations}</ul>
                {link}
            </div>
        </div>
    }
}
