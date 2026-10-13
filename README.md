# 🐧 TuxTrivia

A browser trivia game about the people who built computing — from Babbage and
Lovelace to Unix hackers, language designers, distro founders and the authors
of the CLI tools you use every day.

Rust all the way down: a [Leptos](https://leptos.dev) frontend compiled to
WebAssembly, served as static files. No backend.

## Layout

```
data/people.toml        the roster — edit this to add people
data/portraits.toml     portrait attributions (generated)
crates/core             data model + question generator (pure Rust, tested natively)
crates/web              Leptos CSR frontend (Trunk)
crates/portraits        CLI that fetches free portraits from Wikimedia Commons
```

## Question types

All generated from `people.toml`, so one person entry yields many questions:

| Kind          | Example                                     | Needs              |
|---------------|---------------------------------------------|--------------------|
| Who created   | Who created curl?                           | a creation         |
| What created  | What did Daniel Stenberg create?            | a creation         |
| When          | When did curl first appear?                 | a creation `year`  |
| Guess who     | Clues revealed one by one, fewer = more pts | 2+ `clues`         |
| Who is this   | Portrait shown, pick the name               | a fetched portrait |

Distractors come from the same deck first. Co-creators (Thompson & Ritchie, the
RSA trio) are never offered as wrong answers for each other, and creations that
contain their creator's name ("Turing test", "Bourne shell") are never used in
who/what questions.

## Develop

```sh
rustup target add wasm32-unknown-unknown
cargo install --locked trunk

cargo test --workspace                                   # validates data/people.toml too
cargo run --package tuxtrivia-core --example round 42    # print a round in the terminal
cd crates/web && trunk serve --open                      # http://127.0.0.1:8080
```

Keys in game: `1`–`4` answer, `c` another clue, `Enter` next.

## Portraits

```sh
TUXTRIVIA_CONTACT="https://your.site" cargo run --package tuxtrivia-portraits
cargo run --package tuxtrivia-portraits -- --only stenberg --force
```

For each person with a `wikipedia` title, the tool asks Wikipedia for the
article's lead image **restricted to free licences**, confirms the file is on
Commons, downloads a thumbnail into `crates/web/public/portraits/`, and writes
author + licence to `data/portraits.toml`. The game shows that attribution
under every portrait and on the credits screen (CC BY-SA requires it). Check
the results once by eye — a lead image is usually, but not always, a portrait.

## Deploy behind Traefik

```sh
docker compose up --detach --build
```

Edit the host, entrypoint and cert resolver labels in `compose.yaml` first.
The image is `static-web-server` serving `dist/`; asset URLs are relative, so
a path-prefix route works too.

## Adding people

Append a `[[person]]` block to `data/people.toml` (the header comment documents
every field), then run `cargo test`. The tests fail on duplicate ids, missing
creations, or a clue that contains the person's surname.
