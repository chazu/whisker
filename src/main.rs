//! Collect one information row from the configured view; Bash owns editing.
mod config;

use std::{
    env,
    path::Path,
    process::{self, Command, Stdio},
};
use unicode_width::UnicodeWidthChar;

use config::{Config, Segment, Source, ViewDef};

// These are local metadata commands. No shell evaluation or cluster API calls.
fn output(program: &str, args: &[&str]) -> Option<String> {
    let result = Command::new(program)
        .args(args)
        .env("GIT_OPTIONAL_LOCKS", "0")
        .stdin(Stdio::null())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    result.status.success().then(|| {
        String::from_utf8_lossy(&result.stdout)
            .trim_end()
            .to_owned()
    })
}

fn directory() -> String {
    let actual = env::current_dir().unwrap_or_default();
    // Keep the shell's logical path (including symlinks) when it is valid.
    let cwd = env::var_os("PWD")
        .map(std::path::PathBuf::from)
        .filter(|path| path.is_absolute() && path.canonicalize().ok().as_ref() == Some(&actual))
        .unwrap_or(actual);
    if let Some(home) = env::var_os("HOME")
        && let Ok(relative) = cwd.strip_prefix(Path::new(&home))
    {
        return if relative.as_os_str().is_empty() {
            "~".into()
        } else {
            format!("~/{}", relative.display())
        };
    }
    cwd.display().to_string()
}

fn git() -> Option<String> {
    let status = output(
        "git",
        &[
            "status",
            "--porcelain=v1",
            "--branch",
            "--untracked-files=normal",
        ],
    )?;
    let mut lines = status.lines();
    let header = lines.next()?.strip_prefix("## ")?;
    let branch = if header.starts_with("HEAD (no branch)") {
        format!("@{}", output("git", &["rev-parse", "--short", "HEAD"])?)
    } else {
        header
            .strip_prefix("No commits yet on ")
            .or_else(|| header.strip_prefix("Initial commit on "))
            .unwrap_or(header)
            .split("...")
            .next()?
            .to_owned()
    };
    let dirty = if lines.next().is_some() { "*" } else { "" };
    Some(format!("git:{branch}{dirty}"))
}

fn kubernetes() -> String {
    // Ask kubectl to apply its normal KUBECONFIG merge rules, extracting only
    // the selected context and namespace. This does not run credential plugins.
    let Some(config) = output(
        "kubectl",
        &[
            "config",
            "view",
            "--minify",
            "-o",
            "jsonpath={.current-context}{'\t'}{.contexts[0].context.namespace}",
        ],
    ) else {
        return "⎈ unavailable".into();
    };
    let (context, namespace) = config.split_once('\t').unwrap_or((&config, ""));
    if context.is_empty() {
        return "⎈ none".into();
    }
    let namespace = if namespace.is_empty() {
        "default"
    } else {
        namespace
    };
    format!("⎈ {context}:{namespace}")
}

/// A user-defined segment: run the program directly with no shell, and use its
/// first output line. A missing program or failure hides the segment.
fn custom(argv: &[String]) -> String {
    let args: Vec<&str> = argv[1..].iter().map(String::as_str).collect();
    output(&argv[0], &args)
        .and_then(|text| text.lines().next().map(str::to_owned))
        .unwrap_or_default()
}

/// The segment's body only. The prefix and suffix are applied during layout so
/// that shortening eats into the body and never into the decoration.
fn collect(segment: &Segment) -> String {
    match &segment.source {
        Source::Directory => directory(),
        Source::Git => git().unwrap_or_default(),
        Source::Kubernetes => kubernetes(),
        Source::Command(argv) => custom(argv),
    }
}

// Plain text only. Prevent metadata from inserting terminal controls or rows.
fn clean(text: &str) -> String {
    text.chars()
        .map(|ch| if ch.is_control() { ' ' } else { ch })
        .collect()
}

fn width(text: &str) -> usize {
    text.chars().map(|ch| ch.width().unwrap_or(0)).sum()
}

fn clip(text: &str, budget: usize, keep_end: bool) -> String {
    if width(text) <= budget {
        return text.to_owned();
    }
    if budget == 0 {
        return String::new();
    }
    let mut used = 0;
    let chars: Box<dyn Iterator<Item = char>> = if keep_end {
        Box::new(text.chars().rev())
    } else {
        Box::new(text.chars())
    };
    let mut kept = Vec::new();
    for ch in chars {
        let size = ch.width().unwrap_or(0);
        if used + size > budget - 1 {
            break;
        }
        kept.push(ch);
        used += size;
    }
    if keep_end {
        kept.reverse();
        format!("…{}", kept.into_iter().collect::<String>())
    } else {
        format!("{}…", kept.into_iter().collect::<String>())
    }
}

/// Join the collected parts, shortening shrinkable segments to fit the width.
fn layout(view: &ViewDef, parts: &[String], columns: usize) -> String {
    // Leave one column spare to avoid automatic wrapping on the info row.
    let budget = columns.saturating_sub(1);
    let label = clean(&view.label);
    let separator = clean(&view.separator);
    let mut parts: Vec<String> = parts.iter().map(|part| clean(part)).collect();

    // An empty body drops the segment entirely, decoration and separator too.
    let decorate = |index: usize, body: &str| {
        let segment = &view.segments[index];
        format!("{}{body}{}", clean(&segment.prefix), clean(&segment.suffix))
    };
    let joined = |parts: &[String]| {
        let visible: Vec<String> = parts
            .iter()
            .enumerate()
            .filter(|(_, body)| !body.is_empty())
            .map(|(index, body)| decorate(index, body))
            .collect();
        format!("{label}{}", visible.join(&separator))
    };

    // Shorten shrinkable segments, largest first, until the row fits.
    loop {
        let row = joined(&parts);
        let excess = width(&row).saturating_sub(budget);
        if excess == 0 {
            return row;
        }
        let Some(index) = view
            .segments
            .iter()
            .enumerate()
            .filter(|(index, segment)| segment.shrink && !parts[*index].is_empty())
            .max_by_key(|(index, _)| width(&parts[*index]))
            .map(|(index, _)| index)
        else {
            // Nothing may shrink: cap the whole row instead.
            return clip(&row, budget, false);
        };
        let target = width(&parts[index]).saturating_sub(excess).max(1);
        let shortened = clip(&parts[index], target, view.segments[index].keep_end);
        if shortened == parts[index] {
            return clip(&row, budget, false);
        }
        parts[index] = shortened;
    }
}

fn render(config: &Config, view: &str, columns: usize) -> Result<String, String> {
    let view = config.view(view)?;
    let parts: Vec<String> = view.segments.iter().map(collect).collect();
    Ok(layout(view, &parts, columns))
}

const USAGE: &str = "Whisker\n\n  whisker render [--view NAME] [--columns N]\n  whisker view next --current NAME\n  whisker view list\n  whisker view start\n  whisker config path|check|example\n\nConfiguration: $WHISKER_CONFIG, else ~/.config/whisker/config.toml.\nRun ./try-it for the interactive Bash experiment.";

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "--help" | "-h") {
        println!("{USAGE}");
        return Ok(());
    }

    // `config example` and `config path` must work even when the file is broken.
    if args[0] == "config" {
        return match args.get(1).map(String::as_str) {
            Some("example") => {
                print!("{}", config::DEFAULT_CONFIG);
                Ok(())
            }
            Some("path") => {
                println!(
                    "{}",
                    config::path()
                        .map(|path| path.display().to_string())
                        .unwrap_or_else(|| "<none: set HOME or WHISKER_CONFIG>".into())
                );
                Ok(())
            }
            Some("check") => {
                let loaded = config::load()?;
                let origin = loaded
                    .origin
                    .as_ref()
                    .map(|path| path.display().to_string())
                    .unwrap_or_else(|| "built-in defaults".into());
                println!(
                    "ok: {origin}\nviews: {}\nstart: {}",
                    loaded.view_names(),
                    loaded.start
                );
                Ok(())
            }
            _ => Err("expected config path, check, or example".into()),
        };
    }

    let config = config::load()?;

    if args[0] == "view" {
        return match args.get(1).map(String::as_str) {
            Some("list") => {
                for view in &config.views {
                    println!("{}", view.name);
                }
                Ok(())
            }
            Some("start") => {
                println!("{}", config.start);
                Ok(())
            }
            Some("next") if args.len() == 4 && args[2] == "--current" => {
                println!("{}", config.next(&args[3])?);
                Ok(())
            }
            _ => Err("expected view next --current NAME, view list, or view start".into()),
        };
    }

    if args[0] != "render" {
        return Err("expected render, view, or config; see --help".into());
    }

    let mut view = config.start.clone();
    let mut columns = 80;
    let mut options = args[1..].iter();
    while let Some(option) = options.next() {
        let value = options
            .next()
            .ok_or_else(|| format!("missing value for {option}"))?;
        match option.as_str() {
            "--view" => view = value.clone(),
            "--columns" => {
                columns = value
                    .parse::<usize>()
                    .map_err(|_| "columns must be an integer")?
                    .clamp(1, 16384)
            }
            _ => return Err(format!("unknown option: {option}")),
        }
    }
    println!("{}", render(&config, &view, columns)?);
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("whisker: {error}");
        process::exit(2);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn defaults() -> Config {
        config::parse(config::DEFAULT_CONFIG, None).expect("default config parses")
    }

    fn view(config: &Config, name: &str) -> ViewDef {
        config.view(name).expect("view exists").clone()
    }

    #[test]
    fn default_config_matches_the_original_prototype_cycle() {
        let config = defaults();
        assert_eq!(config.view_names(), "dev, ops, minimal");
        assert_eq!(config.start, "dev");
        assert_eq!(config.next("dev").unwrap(), "ops");
        assert_eq!(config.next("ops").unwrap(), "minimal");
        assert_eq!(config.next("minimal").unwrap(), "dev");
        assert!(config.next("nope").is_err());
    }

    #[test]
    fn default_views_lay_out_like_the_original() {
        let config = defaults();
        assert_eq!(
            layout(&view(&config, "minimal"), &["~/dev".into()], 80),
            "~/dev"
        );
        assert_eq!(
            layout(
                &view(&config, "dev"),
                &["~/dev".into(), "git:main*".into()],
                80
            ),
            "[dev] ~/dev  git:main*"
        );
        assert_eq!(
            layout(
                &view(&config, "ops"),
                &["~/dev".into(), "⎈ staging:payments".into()],
                80
            ),
            "[ops] ~/dev  ⎈ staging:payments"
        );
    }

    #[test]
    fn an_empty_segment_takes_no_separator() {
        let config = defaults();
        assert_eq!(
            layout(&view(&config, "dev"), &["~/dev".into(), String::new()], 80),
            "[dev] ~/dev"
        );
    }

    #[test]
    fn the_directory_shrinks_before_the_detail() {
        let config = defaults();
        let row = layout(
            &view(&config, "dev"),
            &[
                "~/a/very/long/path/to/somewhere/deep".into(),
                "git:main*".into(),
            ],
            30,
        );
        assert!(width(&row) <= 29, "row {row:?} exceeds the budget");
        assert!(row.ends_with("git:main*"), "detail lost in {row:?}");
        assert!(row.starts_with("[dev] …"), "wrong end kept in {row:?}");
    }

    #[test]
    fn shortening_eats_the_body_and_keeps_the_decoration() {
        let config = config::parse(
            r#"
views = ["a"]
[view.a]
segments = ["directory"]
[segment.directory]
prefix = "> "
suffix = " <"
"#,
            None,
        )
        .unwrap();
        let row = layout(
            &view(&config, "a"),
            &["~/a/very/long/path/to/somewhere/deep".into()],
            20,
        );
        assert!(width(&row) <= 19, "row {row:?} exceeds the budget");
        assert!(row.starts_with("> …"), "prefix lost in {row:?}");
        assert!(row.ends_with(" <"), "suffix lost in {row:?}");
    }

    #[test]
    fn a_narrow_terminal_caps_the_whole_row() {
        let config = defaults();
        let row = layout(
            &view(&config, "dev"),
            &["~/somewhere".into(), "git:a-very-long-branch-name".into()],
            12,
        );
        assert!(width(&row) <= 11, "row {row:?} exceeds the budget");
    }

    #[test]
    fn control_characters_never_reach_the_row() {
        let config = defaults();
        let row = layout(
            &view(&config, "dev"),
            &["~/dev".into(), "git:ma\nin\u{1b}[31m".into()],
            80,
        );
        assert!(!row.contains('\n') && !row.contains('\u{1b}'), "{row:?}");
    }

    #[test]
    fn a_custom_view_and_segment_render() {
        let config = config::parse(
            r#"
views = ["cloud"]
[view.cloud]
label = "» "
separator = " | "
segments = ["directory", "region"]
[segment.region]
command = ["echo", "us-east-1"]
prefix = "aws:"
"#,
            None,
        )
        .expect("custom config parses");
        assert_eq!(config.start, "cloud");
        assert_eq!(config.next("cloud").unwrap(), "cloud");
        assert_eq!(
            render(&config, "cloud", 200).unwrap(),
            format!("» {} | aws:us-east-1", directory())
        );
    }

    #[test]
    fn a_failing_custom_segment_disappears() {
        let config = config::parse(
            r#"
views = ["x"]
[view.x]
segments = ["directory", "gone"]
[segment.gone]
command = ["whisker-no-such-program-exists"]
prefix = "!"
"#,
            None,
        )
        .unwrap();
        assert_eq!(render(&config, "x", 200).unwrap(), directory());
    }

    #[test]
    fn configuration_mistakes_are_reported() {
        let cases = [
            ("views = []", "at least one"),
            ("views = [\"a\"]", "[view.a] is missing"),
            ("views = [\"a\"]\n[view.a]\n", "must set segments"),
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"nope\"]",
                "unknown segment",
            ),
            (
                "views = [\"a\",\"a\"]\n[view.a]\nsegments = [\"git\"]",
                "listed twice",
            ),
            (
                "views = [\"a\"]\nstart = \"b\"\n[view.a]\nsegments = [\"git\"]",
                "not a configured view",
            ),
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"git\"]\ncolour = \"red\"",
                "unknown key colour",
            ),
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"s\"]\n[segment.s]\nprefix = \"p\"",
                "must set command",
            ),
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"git\"]\n[segment.git]\ncommand = [\"x\"]",
                "built in",
            ),
            ("views = [\"a\"] [", "TOML"),
        ];
        for (text, expected) in cases {
            let error = config::parse(text, None).expect_err(&format!("{text:?} should fail"));
            assert!(
                error.to_lowercase().contains(&expected.to_lowercase()),
                "{text:?} gave {error:?}, wanted {expected:?}"
            );
        }
    }
}
