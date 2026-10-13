//! Fetches a freely licensed portrait for each person in data/people.toml.
//!
//! For each person with a `wikipedia` title it asks Wikipedia for the
//! article's lead image *restricted to free licences*, checks the file
//! lives on Wikimedia Commons, downloads a thumbnail into
//! crates/web/public/portraits/ and records author + licence in
//! data/portraits.toml, which the game shows next to every portrait.
//!
//! With `--allow-non-free` it also takes non-free lead images (fair use,
//! hosted only on English Wikipedia) and marks them `non_free = true`.
//!
//! Usage:
//!   cargo run --package tuxtrivia-portraits -- [--force] [--allow-non-free] [--only <id>]... [--size <px>]
//!
//! Wikimedia asks API clients to identify themselves; the User-Agent carries
//! the repository URL, or TUXTRIVIA_CONTACT (a URL or email) when set.

use std::path::{Path, PathBuf};
use std::time::Duration;
use std::{env, fs, process, thread};

use serde_json::Value;
use tuxtrivia_core::{Person, Portrait, PortraitsFile, bundled_roster};

const WIKIPEDIA_API: &str = "https://en.wikipedia.org/w/api.php";
const COMMONS_API: &str = "https://commons.wikimedia.org/w/api.php";
const REPO_URL: &str = "https://github.com/coko7/tuxtrivia";
const PAUSE: Duration = Duration::from_secs(1);
const RETRIES: u32 = 5;

/// Thumbnail widths Wikimedia renders on demand. Other widths are throttled
/// (HTTP 429) or refused: see https://w.wiki/GHai
const THUMB_STEPS: [u32; 11] = [20, 40, 60, 120, 250, 330, 500, 960, 1280, 1920, 3840];

/// Smallest standard width at least `px` wide.
fn thumb_step(px: u32) -> u32 {
    THUMB_STEPS
        .into_iter()
        .find(|&step| step >= px)
        .unwrap_or(THUMB_STEPS[THUMB_STEPS.len() - 1])
}

struct Args {
    force: bool,
    allow_non_free: bool,
    only: Vec<String>,
    size: u32,
}

fn parse_args() -> Args {
    let mut args = Args {
        force: false,
        allow_non_free: false,
        only: Vec::new(),
        size: 500,
    };
    let mut it = env::args().skip(1);
    while let Some(arg) = it.next() {
        match arg.as_str() {
            "--force" => args.force = true,
            "--allow-non-free" => args.allow_non_free = true,
            "--only" => args
                .only
                .push(it.next().unwrap_or_else(|| usage("--only needs an id"))),
            "--size" => {
                args.size = it
                    .next()
                    .and_then(|s| s.parse().ok())
                    .unwrap_or_else(|| usage("--size needs a number of pixels"))
            }
            "--help" => usage(""),
            other => usage(&format!("unknown argument `{other}`")),
        }
    }
    args
}

fn usage(error: &str) -> ! {
    if !error.is_empty() {
        eprintln!("error: {error}\n");
    }
    eprintln!(
        "usage: tuxtrivia-portraits [--force] [--allow-non-free] [--only <id>]... [--size <px>]"
    );
    eprintln!("  --force           re-download portraits that already exist");
    eprintln!("  --allow-non-free  also take non-free (fair use) images, marked `non_free`");
    eprintln!("  --only <id>       only fetch this person (repeatable)");
    eprintln!(
        "  --size <px>       thumbnail width, rounded up to a Wikimedia standard size (default 500)"
    );
    process::exit(if error.is_empty() { 0 } else { 2 });
}

fn main() {
    let mut args = parse_args();
    args.size = thumb_step(args.size);
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    let manifest_path = root.join("data/portraits.toml");
    let image_dir = root.join("crates/web/public/portraits");
    fs::create_dir_all(&image_dir).expect("create portraits directory");

    let roster = bundled_roster().unwrap_or_else(|e| fail(&format!("data/people.toml: {e}")));
    let mut manifest: PortraitsFile = fs::read_to_string(&manifest_path)
        .ok()
        .map(|s| toml::from_str(&s).unwrap_or_else(|e| fail(&format!("data/portraits.toml: {e}"))))
        .unwrap_or_default();

    let agent = ureq::Agent::new_with_defaults();
    // Wikimedia throttles clients without contact info much harder.
    let contact = env::var("TUXTRIVIA_CONTACT").unwrap_or_else(|_| REPO_URL.to_string());
    let user_agent = format!(
        "TuxTrivia-portraits/{} ({contact})",
        env!("CARGO_PKG_VERSION")
    );

    let (mut fetched, mut skipped, mut missing, mut failed) = (0, 0, 0, 0);
    for person in roster.people() {
        if !args.only.is_empty() && !args.only.contains(&person.id) {
            continue;
        }
        let Some(title) = &person.wikipedia else {
            continue;
        };

        let existing = manifest.portrait.iter().position(|p| p.id == person.id);
        if let Some(i) = existing
            && !args.force
            && image_dir.join(&manifest.portrait[i].file).exists()
        {
            skipped += 1;
            continue;
        }

        match fetch(&agent, &user_agent, person, title, &args, &image_dir) {
            Ok(Fetched::Found(portrait)) => {
                println!(
                    "✓ {:<22} {}{} — {}",
                    person.id,
                    portrait.license,
                    if portrait.non_free { " (NON-FREE)" } else { "" },
                    portrait.artist
                );
                // A new extension leaves the old file behind.
                if let Some(i) = existing
                    && manifest.portrait[i].file != portrait.file
                {
                    let _ = fs::remove_file(image_dir.join(&manifest.portrait[i].file));
                }
                match existing {
                    Some(i) => manifest.portrait[i] = portrait,
                    None => manifest.portrait.push(portrait),
                }
                fetched += 1;
            }
            Ok(Fetched::NoPortrait(reason)) => {
                println!("· {:<22} {reason}", person.id);
                if let Some(i) = existing {
                    let stale = manifest.portrait.remove(i);
                    let _ = fs::remove_file(image_dir.join(&stale.file));
                    println!("  removed previous portrait `{}`", stale.file);
                }
                missing += 1;
            }
            Err(reason) => {
                println!("✗ {:<22} {reason}", person.id);
                failed += 1;
            }
        }
        thread::sleep(PAUSE);
    }

    // Keep the manifest in roster order so diffs stay readable.
    let order = |id: &str| {
        roster
            .people()
            .iter()
            .position(|p| p.id == id)
            .unwrap_or(usize::MAX)
    };
    manifest.portrait.sort_by_key(|p| order(&p.id));
    let body = toml::to_string(&manifest).expect("serialise portraits");
    let header =
        "# Generated by `cargo run --package tuxtrivia-portraits`. Do not edit by hand.\n\n";
    fs::write(&manifest_path, format!("{header}{body}")).expect("write data/portraits.toml");

    println!(
        "\n{fetched} fetched, {skipped} already present, {missing} without a free portrait, {failed} failed."
    );
    if failed > 0 {
        println!("Run again to retry the failures.");
    }
    if fetched > 0 {
        println!("Rebuild the game (trunk build) to bundle the new attributions.");
    }
}

fn fail(msg: &str) -> ! {
    eprintln!("error: {msg}");
    process::exit(1);
}

/// Runs `request`, backing off and retrying while Wikimedia answers 429.
fn with_retry<T>(mut request: impl FnMut() -> Result<T, ureq::Error>) -> Result<T, ureq::Error> {
    let mut wait = Duration::from_secs(2);
    for _ in 0..RETRIES {
        match request() {
            Err(ureq::Error::StatusCode(429)) => {
                eprintln!("  rate limited, retrying in {}s", wait.as_secs());
                thread::sleep(wait);
                wait *= 2;
            }
            result => return result,
        }
    }
    request()
}

enum Fetched {
    Found(Portrait),
    /// Wikipedia has no usable free portrait of this person.
    NoPortrait(String),
}

/// `Err` is a transient failure (network, rate limit, disk) that leaves any
/// existing portrait in place.
fn fetch(
    agent: &ureq::Agent,
    user_agent: &str,
    person: &Person,
    title: &str,
    args: &Args,
    image_dir: &Path,
) -> Result<Fetched, String> {
    let size = args.size;
    let pilicense = if args.allow_non_free { "any" } else { "free" };
    // 1. The article's lead image, only if it is freely licensed (unless
    //    --allow-non-free).
    let page: Value = with_retry(|| {
        agent
            .get(WIKIPEDIA_API)
            .header("User-Agent", user_agent)
            .query("action", "query")
            .query("format", "json")
            .query("formatversion", "2")
            .query("redirects", "1")
            .query("prop", "pageimages")
            .query("piprop", "thumbnail|name")
            .query("pilicense", pilicense)
            .query("pithumbsize", size.to_string())
            .query("titles", title)
            .call()
            .and_then(|mut r| r.body_mut().read_json())
    })
    .map_err(|e| format!("Wikipedia request failed: {e}"))?;

    let none = |reason: String| Ok(Fetched::NoPortrait(reason));
    let page = &page["query"]["pages"][0];
    if page["missing"].as_bool() == Some(true) {
        return none(format!("no Wikipedia article titled `{title}`"));
    }
    let Some(file) = page["pageimage"].as_str() else {
        return none(format!("article has no {pilicense} lead image"));
    };
    // Articles without a photo often lead with a project logo, always SVG.
    if file.to_ascii_lowercase().ends_with(".svg") {
        return none(format!("lead image `{file}` is a drawing, likely a logo"));
    }
    let Some(thumb_url) = page["thumbnail"]["source"].as_str() else {
        return none("no thumbnail available".into());
    };

    // 2. Authorship and licence from Commons. Files missing there are local
    //    to English Wikipedia, which usually means non-free: skip them,
    //    unless --allow-non-free, then read their English Wikipedia page.
    let image_info = |api: &str| {
        with_retry(|| {
            agent
                .get(api)
                .header("User-Agent", user_agent)
                .query("action", "query")
                .query("format", "json")
                .query("formatversion", "2")
                .query("prop", "imageinfo")
                .query("iiprop", "extmetadata|url")
                .query("titles", format!("File:{file}"))
                .call()
                .and_then(|mut r| r.body_mut().read_json::<Value>())
        })
        .map(|mut info| {
            info.pointer_mut("/query/pages/0")
                .map(Value::take)
                .unwrap_or_default()
        })
    };
    let mut file_page =
        image_info(COMMONS_API).map_err(|e| format!("Commons request failed: {e}"))?;
    let on_commons = file_page["missing"].as_bool() != Some(true);
    if !on_commons {
        if !args.allow_non_free {
            return none(format!("`{file}` is not on Commons (likely non-free)"));
        }
        file_page =
            image_info(WIKIPEDIA_API).map_err(|e| format!("Wikipedia request failed: {e}"))?;
        if file_page["missing"].as_bool() == Some(true) {
            return none(format!("`{file}` is on neither Commons nor Wikipedia"));
        }
    }
    let image = &file_page["imageinfo"][0];
    let meta = &image["extmetadata"];
    let field = |name: &str| {
        meta[name]["value"]
            .as_str()
            .map(clean_html)
            .filter(|s| !s.is_empty())
    };

    let Some(license) = field("LicenseShortName") else {
        return none("Commons file has no licence metadata".into());
    };
    let artist = field("Artist").unwrap_or_else(|| "Unknown author".to_string());
    let license_url = field("LicenseUrl");
    // Some free files live only on English Wikipedia, so where a file is
    // hosted says nothing: trust the NonFree flag set by fair-use templates.
    let non_free = field("NonFree").as_deref() == Some("true");
    let source_url = image["descriptionurl"]
        .as_str()
        .map(str::to_string)
        .unwrap_or_else(|| {
            let host = if on_commons {
                "commons.wikimedia.org"
            } else {
                "en.wikipedia.org"
            };
            format!("https://{host}/wiki/File:{}", file.replace(' ', "_"))
        });

    // 3. Download the thumbnail. Its URL may carry a query string.
    let ext = thumb_url
        .split('?')
        .next()
        .unwrap_or(thumb_url)
        .rsplit('/')
        .next()
        .and_then(|name| name.rsplit_once('.'))
        .map(|(_, ext)| ext.to_ascii_lowercase())
        .filter(|ext| matches!(ext.as_str(), "jpg" | "jpeg" | "png" | "webp" | "gif"))
        .unwrap_or_else(|| "jpg".to_string());
    let bytes = with_retry(|| {
        agent
            .get(thumb_url)
            .header("User-Agent", user_agent)
            .call()
            .and_then(|mut r| r.body_mut().read_to_vec())
    })
    .map_err(|e| format!("download failed: {e}"))?;

    let file_name = format!("{}.{ext}", person.id);
    fs::write(image_dir.join(&file_name), bytes).map_err(|e| format!("write failed: {e}"))?;

    Ok(Fetched::Found(Portrait {
        id: person.id.clone(),
        file: file_name,
        artist,
        license,
        license_url,
        source_url,
        non_free,
    }))
}

/// Commons metadata is HTML ("<a href=...>Jane Doe</a>"). Keep the visible
/// text: templates often repeat it in a `display: none` span.
fn clean_html(html: &str) -> String {
    let mut text = String::with_capacity(html.len());
    let mut rest = html;
    // Elements open inside a hidden one, counting itself; 0 means visible.
    let mut hidden = 0usize;
    while let Some(start) = rest.find('<') {
        if hidden == 0 {
            text.push_str(&rest[..start]);
        }
        let Some(len) = rest[start..].find('>') else {
            rest = "";
            break;
        };
        let tag = &rest[start + 1..start + len];
        rest = &rest[start + len + 1..];

        let name = tag
            .trim_start_matches('/')
            .split(|c: char| c.is_whitespace() || c == '/')
            .next()
            .unwrap_or("")
            .to_ascii_lowercase();
        let void = tag.ends_with('/') || matches!(name.as_str(), "br" | "hr" | "img" | "wbr");
        if tag.starts_with('/') {
            hidden = hidden.saturating_sub(1);
        } else if !void
            && (hidden > 0
                || tag
                    .replace(' ', "")
                    .to_ascii_lowercase()
                    .contains("display:none"))
        {
            hidden += 1;
        }
    }
    if hidden == 0 {
        text.push_str(rest);
    }
    let text = text
        .replace("&amp;", "&")
        .replace("&quot;", "\"")
        .replace("&#039;", "'")
        .replace("&#39;", "'")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&nbsp;", " ");
    text.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[cfg(test)]
mod tests {
    use super::{clean_html, thumb_step};

    #[test]
    fn rounds_up_to_standard_thumb_width() {
        assert_eq!(thumb_step(400), 500);
        assert_eq!(thumb_step(500), 500);
        assert_eq!(thumb_step(1), 20);
        assert_eq!(thumb_step(10_000), 3840);
    }

    #[test]
    fn strips_commons_artist_markup() {
        let html = "<a href=\"//commons.wikimedia.org/wiki/User:X\" title=\"User:X\">Jane&nbsp;Doe</a>\n (photo)";
        assert_eq!(clean_html(html), "Jane Doe (photo)");
    }

    #[test]
    fn drops_hidden_duplicate_text() {
        let html = "Unknown author<span style=\"display: none;\">Unknown author</span>";
        assert_eq!(clean_html(html), "Unknown author");
        let nested = "<span style=\"display:none\"><b>x</b><br>y</span>Ada<br/> Lovelace";
        assert_eq!(clean_html(nested), "Ada Lovelace");
    }
}
