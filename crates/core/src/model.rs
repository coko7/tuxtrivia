//! The people roster: who they are, what they made, and their portraits.

use std::collections::{HashMap, HashSet};
use std::fmt;

use serde::{Deserialize, Serialize};

/// Broad bucket a person belongs to. Used to pick a deck and to choose
/// plausible distractors (a Unix pioneer is a better wrong answer to
/// "who wrote vi?" than a distro maintainer).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, PartialOrd, Ord, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Category {
    Pioneers,
    Unix,
    Languages,
    Distros,
    Tools,
}

impl Category {
    pub const ALL: [Category; 5] = [
        Category::Pioneers,
        Category::Unix,
        Category::Languages,
        Category::Distros,
        Category::Tools,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Category::Pioneers => "Pioneers & theory",
            Category::Unix => "Unix, C & free software",
            Category::Languages => "Language creators",
            Category::Distros => "Distros & operating systems",
            Category::Tools => "CLI, tools & infrastructure",
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Deserialize, Serialize)]
#[serde(rename_all = "kebab-case")]
pub enum Kind {
    Language,
    Os,
    Distro,
    Tool,
    Software,
    Algorithm,
    Concept,
    Hardware,
    Book,
    Protocol,
    Format,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Creation {
    pub name: String,
    /// First public release / publication. Leave out when it is fuzzy;
    /// creations without a year are never used in "when" questions.
    #[serde(default)]
    pub year: Option<i32>,
    pub kind: Kind,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Person {
    /// Stable slug, also the portrait file stem.
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub born: Option<i32>,
    #[serde(default)]
    pub country: Option<String>,
    /// English Wikipedia article about the person; its free lead image
    /// becomes the portrait. Leave out if there is no such article.
    #[serde(default)]
    pub wikipedia: Option<String>,
    pub category: Category,
    pub creations: Vec<Creation>,
    /// Ordered from vaguest to most revealing, for "guess who" rounds.
    /// Clues must not contain the person's name.
    #[serde(default)]
    pub clues: Vec<String>,
}

/// Attribution for a portrait downloaded from Wikimedia Commons (or, when
/// `non_free`, from English Wikipedia).
/// Written by the `tuxtrivia-portraits` tool, never by hand.
#[derive(Debug, Clone, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Portrait {
    pub id: String,
    /// File name inside `crates/web/public/portraits/`.
    pub file: String,
    pub artist: String,
    pub license: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub license_url: Option<String>,
    /// File description page, on Commons or on English Wikipedia.
    pub source_url: String,
    /// Not freely licensed (e.g. fair use), so only hosted on English
    /// Wikipedia. Fetched with `--allow-non-free`.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub non_free: bool,
}

#[derive(Deserialize)]
struct PeopleFile {
    person: Vec<Person>,
}

#[derive(Deserialize, Serialize, Default)]
pub struct PortraitsFile {
    #[serde(default)]
    pub portrait: Vec<Portrait>,
}

#[derive(Debug)]
pub enum LoadError {
    Toml(toml::de::Error),
    DuplicateId(String),
    NoCreations(String),
    UnknownPortrait(String),
}

impl fmt::Display for LoadError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LoadError::Toml(e) => write!(f, "invalid TOML: {e}"),
            LoadError::DuplicateId(id) => write!(f, "duplicate person id `{id}`"),
            LoadError::NoCreations(id) => write!(f, "person `{id}` has no creations"),
            LoadError::UnknownPortrait(id) => write!(f, "portrait for unknown person `{id}`"),
        }
    }
}

impl std::error::Error for LoadError {}

impl From<toml::de::Error> for LoadError {
    fn from(e: toml::de::Error) -> Self {
        LoadError::Toml(e)
    }
}

/// Index of a person inside [`Roster::people`].
pub type PersonIdx = usize;

pub struct Roster {
    people: Vec<Person>,
    portraits: HashMap<String, Portrait>,
    /// Lower-cased creation name → everyone credited with it. Keeps
    /// co-creators (Thompson & Ritchie, the RSA trio) out of each
    /// other's distractor lists.
    credited: HashMap<String, HashSet<PersonIdx>>,
}

pub(crate) fn key(name: &str) -> String {
    name.trim().to_lowercase()
}

/// Lower-cased last word of a display name: "Edsger W. Dijkstra" → "dijkstra".
pub fn surname(name: &str) -> String {
    name.split_whitespace()
        .last()
        .unwrap_or(name)
        .trim_matches('"')
        .to_lowercase()
}

impl Roster {
    pub fn from_toml(people: &str, portraits: &str) -> Result<Self, LoadError> {
        let people = toml::from_str::<PeopleFile>(people)?.person;
        let portraits_file: PortraitsFile = toml::from_str(portraits)?;

        let mut seen = HashSet::new();
        let mut credited: HashMap<String, HashSet<PersonIdx>> = HashMap::new();
        for (idx, p) in people.iter().enumerate() {
            if !seen.insert(p.id.as_str()) {
                return Err(LoadError::DuplicateId(p.id.clone()));
            }
            if p.creations.is_empty() {
                return Err(LoadError::NoCreations(p.id.clone()));
            }
            for c in &p.creations {
                credited.entry(key(&c.name)).or_default().insert(idx);
            }
        }

        let mut portraits = HashMap::new();
        for portrait in portraits_file.portrait {
            if !seen.contains(portrait.id.as_str()) {
                return Err(LoadError::UnknownPortrait(portrait.id));
            }
            portraits.insert(portrait.id.clone(), portrait);
        }

        Ok(Roster {
            people,
            portraits,
            credited,
        })
    }

    pub fn people(&self) -> &[Person] {
        &self.people
    }

    pub fn person(&self, idx: PersonIdx) -> &Person {
        &self.people[idx]
    }

    pub fn portrait(&self, idx: PersonIdx) -> Option<&Portrait> {
        self.portraits.get(&self.people[idx].id)
    }

    pub fn portraits(&self) -> impl Iterator<Item = (&Person, &Portrait)> {
        self.people
            .iter()
            .filter_map(|p| self.portraits.get(&p.id).map(|portrait| (p, portrait)))
    }

    /// Everyone credited with a creation of this name.
    pub fn credited_with(&self, creation: &str) -> Option<&HashSet<PersonIdx>> {
        self.credited.get(&key(creation))
    }

    /// True if the creation carries a creator's name ("Turing test",
    /// "Bourne shell", "Diffie–Hellman"). Those would give the answer away.
    pub fn is_eponymous(&self, creation: &str) -> bool {
        let creation = key(creation);
        self.credited_with(&creation).is_some_and(|people| {
            people
                .iter()
                .any(|&i| creation.contains(&surname(&self.people[i].name)))
        })
    }

    pub fn count_in(&self, categories: &[Category]) -> usize {
        self.people
            .iter()
            .filter(|p| categories.contains(&p.category))
            .count()
    }
}
