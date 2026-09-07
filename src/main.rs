//! Collect one information row from the configured view; Bash owns editing.
mod config;
mod style;

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
    collect_source(&segment.source)
}

fn collect_source(source: &Source) -> String {
    match source {
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
fn layout(view: &ViewDef, parts: &[String], columns: usize, color: bool) -> String {
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
    // Measured on plain text only. Style is applied once the row is final, so
    // an escape sequence is never counted as a column nor cut by shortening.
    let joined = |parts: &[String]| {
        let visible: Vec<String> = parts
            .iter()
            .enumerate()
            .filter(|(_, body)| !body.is_empty())
            .map(|(index, body)| decorate(index, body))
            .collect();
        format!("{label}{}", visible.join(&separator))
    };
    let painted = |parts: &[String]| {
        if !color {
            return joined(parts);
        }
        let visible: Vec<String> = parts
            .iter()
            .enumerate()
            .filter(|(_, body)| !body.is_empty())
            .map(|(index, body)| view.segments[index].style.paint(&decorate(index, body)))
            .collect();
        format!(
            "{}{}",
            view.label_style.paint(&label),
            visible.join(&view.separator_style.paint(&separator))
        )
    };

    // Shorten shrinkable segments, largest first, until the row fits.
    // `degraded` records atomic segments that have already fallen back, so a
    // fallback that is itself too wide is dropped rather than reconsidered
    // forever.
    let mut degraded = vec![false; view.segments.len()];
    loop {
        let excess = width(&joined(&parts)).saturating_sub(budget);
        if excess == 0 {
            return painted(&parts);
        }
        let Some(index) = view
            .segments
            .iter()
            .enumerate()
            .filter(|(index, segment)| segment.shrink && !parts[*index].is_empty())
            .max_by_key(|(index, _)| width(&parts[*index]))
            .map(|(index, _)| index)
        else {
            // Nothing may shrink. Before clipping the row, give up on any
            // `atomic` segment: it would rather be absent than truncated, and
            // dropping the widest one may be enough to make the row fit.
            if let Some(index) = widest_atomic(view, &parts, &degraded) {
                parts[index] = degrade(&view.segments[index], &mut degraded[index]);
                continue;
            }
            // Cap the plain text and restyle, so a clip can never land inside
            // an escape sequence.
            return capped(view, &parts, budget, color);
        };
        let target = width(&parts[index]).saturating_sub(excess).max(1);
        let shortened = clip(&parts[index], target, view.segments[index].keep_end);
        if shortened == parts[index] {
            // This segment cannot give any more. An atomic segment elsewhere
            // may still be able to, so try that before capping the row.
            if let Some(index) = widest_atomic(view, &parts, &degraded) {
                parts[index] = degrade(&view.segments[index], &mut degraded[index]);
                continue;
            }
            return capped(view, &parts, budget, color);
        }
        parts[index] = shortened;
    }
}

/// The widest `atomic` segment still showing something it can give up.
///
/// Widest first, because dropping the largest offender recovers the most room
/// per segment sacrificed. A segment already showing its fallback is skipped
/// only once that fallback has also been rejected.
fn widest_atomic(view: &ViewDef, parts: &[String], degraded: &[bool]) -> Option<usize> {
    view.segments
        .iter()
        .enumerate()
        .filter(|(index, segment)| {
            segment.atomic && !parts[*index].is_empty() && !degraded[*index]
        })
        .max_by_key(|(index, _)| width(&parts[*index]))
        .map(|(index, _)| index)
}

/// What an `atomic` segment shows once it admits it does not fit: its
/// fallback the first time, nothing the second.
///
/// The fallback is collected only at this point, so the common case where
/// everything fits never pays for a second command.
fn degrade(segment: &Segment, spent: &mut bool) -> String {
    match &segment.fallback {
        Some(source) if !*spent => {
            *spent = true;
            clean(&collect_source(source))
        }
        _ => {
            *spent = true;
            String::new()
        }
    }
}

/// Last resort when nothing may shrink: clip the whole plain row. Any style is
/// dropped rather than risk cutting an escape sequence in half.
fn capped(view: &ViewDef, parts: &[String], budget: usize, color: bool) -> String {
    let label = clean(&view.label);
    let separator = clean(&view.separator);
    let visible: Vec<String> = parts
        .iter()
        .enumerate()
        .filter(|(_, body)| !body.is_empty())
        .map(|(index, body)| {
            let segment = &view.segments[index];
            format!("{}{body}{}", clean(&segment.prefix), clean(&segment.suffix))
        })
        .collect();
    let row = clip(
        &format!("{label}{}", visible.join(&separator)),
        budget,
        false,
    );
    if color {
        // One uniform style keeps the escape sequences whole.
        return view.label_style.paint(&row);
    }
    row
}

fn render(config: &Config, view: &str, columns: usize, color: bool) -> Result<String, String> {
    let view = config.view(view)?;
    let parts: Vec<String> = view.segments.iter().map(collect).collect();
    Ok(layout(view, &parts, columns, color))
}

const USAGE: &str = "Whisker\n\n  whisker render [--view NAME] [--columns N] [--color auto|always|never]\n  whisker view next --current NAME\n  whisker view list\n  whisker view start\n  whisker config path|check|example\n\nConfiguration: $WHISKER_CONFIG, else ~/.config/whisker/config.toml.\nColour defaults to auto: on unless a non-empty NO_COLOR or TERM=dumb is set. The output is\ncaptured by the shell, so auto cannot detect a terminal; use --color never to\nbe certain.\nRun ./try-it for the interactive Bash experiment.";

/// Whisker's output is captured into a shell variable and printed later, so a
/// TTY check here would always say "not a terminal". Auto therefore honours the
/// usual opt-outs and otherwise assumes the row reaches a terminal.
fn color_auto() -> bool {
    // The NO_COLOR standard counts the variable only when it is present and
    // not empty, regardless of its value. An empty NO_COLOR= is not an opt-out.
    if env::var_os("NO_COLOR").is_some_and(|value| !value.is_empty()) {
        return false;
    }
    !matches!(env::var("TERM").as_deref(), Ok("dumb") | Ok(""))
}

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
    let mut color = color_auto();
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
            "--color" | "--colour" => {
                color = match value.as_str() {
                    "always" => true,
                    "never" => false,
                    "auto" => color_auto(),
                    _ => return Err(format!("color must be auto, always, or never, not {value}")),
                }
            }
            _ => return Err(format!("unknown option: {option}")),
        }
    }
    println!("{}", render(&config, &view, columns, color)?);
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
            layout(&view(&config, "minimal"), &["~/dev".into()], 80, false),
            "~/dev"
        );
        assert_eq!(
            layout(
                &view(&config, "dev"),
                &["~/dev".into(), "git:main*".into()],
                80,
                false,
            ),
            "[dev] ~/dev  git:main*"
        );
        assert_eq!(
            layout(
                &view(&config, "ops"),
                &["~/dev".into(), "⎈ staging:payments".into()],
                80,
                false,
            ),
            "[ops] ~/dev  ⎈ staging:payments"
        );
    }

    #[test]
    fn an_empty_segment_takes_no_separator() {
        let config = defaults();
        assert_eq!(
            layout(
                &view(&config, "dev"),
                &["~/dev".into(), String::new()],
                80,
                false
            ),
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
            false,
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
            false,
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
            false,
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
            false,
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
            render(&config, "cloud", 200, false).unwrap(),
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
        assert_eq!(render(&config, "x", 200, false).unwrap(), directory());
    }

    /// Strip SGR sequences, so a styled row can be compared against a plain one.
    fn strip(text: &str) -> String {
        let mut out = String::new();
        let mut chars = text.chars();
        while let Some(ch) = chars.next() {
            if ch == '\u{1b}' {
                for next in chars.by_ref() {
                    if next == 'm' {
                        break;
                    }
                }
            } else {
                out.push(ch);
            }
        }
        out
    }

    fn styled() -> Config {
        config::parse(
            r#"
views = ["a"]
[view.a]
label = "L "
label_style = { fg = "blue", bold = true }
segments = ["directory", "git"]
style = { dim = true }
[segment.git]
style = { fg = "red" }
prefix = "@"
"#,
            None,
        )
        .unwrap()
    }

    #[test]
    fn style_never_changes_the_laid_out_text() {
        let config = styled();
        let parts = ["~/dev".to_string(), "main".to_string()];
        for columns in [80, 40, 20, 14, 8, 3] {
            let plain = layout(&view(&config, "a"), &parts, columns, false);
            let color = layout(&view(&config, "a"), &parts, columns, true);
            assert_eq!(
                strip(&color),
                plain,
                "colour changed the text at {columns} columns"
            );
            assert!(
                width(&plain) <= columns.saturating_sub(1),
                "row {plain:?} exceeds {columns} columns"
            );
        }
    }

    #[test]
    fn a_styled_row_never_ends_mid_escape() {
        let config = styled();
        let parts = [
            "~/a/very/long/path/to/somewhere/deep".to_string(),
            "main".to_string(),
        ];
        for columns in [80, 40, 24, 16, 10, 6, 4, 2] {
            let row = layout(&view(&config, "a"), &parts, columns, true);
            let escapes = row.matches('\u{1b}').count();
            let terminators = row.matches('m').count();
            assert!(
                escapes <= terminators,
                "unterminated escape at {columns} columns in {row:?}"
            );
            if row.contains('\u{1b}') {
                assert!(
                    row.ends_with("\u{1b}[0m"),
                    "style leaks at {columns}: {row:?}"
                );
            }
        }
    }

    #[test]
    fn a_segment_style_beats_the_view_style_and_decoration_is_styled() {
        let config = styled();
        let row = layout(
            &view(&config, "a"),
            &["~/dev".into(), "main".into()],
            80,
            true,
        );
        // The label is blue+bold, the directory inherits dim, git is red+dim.
        assert!(row.starts_with("\u{1b}[0;1;34mL \u{1b}[0m"), "{row:?}");
        assert!(row.contains("\u{1b}[0;2;31m@main\u{1b}[0m"), "{row:?}");
        assert!(row.contains("\u{1b}[0;2m~/dev\u{1b}[0m"), "{row:?}");
    }

    #[test]
    fn colour_is_absent_without_styles_or_when_disabled() {
        let plain = defaults();
        let row = layout(
            &view(&plain, "dev"),
            &["~/dev".into(), "git:main".into()],
            80,
            true,
        );
        assert!(
            !row.contains('\u{1b}'),
            "unstyled config emitted colour: {row:?}"
        );

        let config = styled();
        let off = layout(
            &view(&config, "a"),
            &["~/dev".into(), "main".into()],
            80,
            false,
        );
        assert!(!off.contains('\u{1b}'), "colour emitted when off: {off:?}");
    }

    /// The NO_COLOR standard counts the variable only when present and not
    /// empty. These mutate the process environment, so they run in one test.
    #[test]
    fn color_auto_follows_the_no_color_standard() {
        let restore = (env::var_os("NO_COLOR"), env::var_os("TERM"));
        unsafe {
            env::set_var("TERM", "xterm-256color");

            env::remove_var("NO_COLOR");
            assert!(color_auto(), "colour should be on with no opt-out");

            env::set_var("NO_COLOR", "1");
            assert!(!color_auto(), "NO_COLOR=1 must disable colour");

            // Any non-empty value counts, regardless of what it says.
            env::set_var("NO_COLOR", "0");
            assert!(!color_auto(), "NO_COLOR=0 must still disable colour");
            env::set_var("NO_COLOR", "false");
            assert!(!color_auto(), "NO_COLOR=false must still disable colour");

            // An empty value is not an opt-out.
            env::set_var("NO_COLOR", "");
            assert!(color_auto(), "empty NO_COLOR= must not disable colour");

            env::remove_var("NO_COLOR");
            env::set_var("TERM", "dumb");
            assert!(!color_auto(), "TERM=dumb must disable colour");
            env::set_var("TERM", "");
            assert!(!color_auto(), "an empty TERM must disable colour");
            env::remove_var("TERM");
            assert!(color_auto(), "an unset TERM should still allow colour");

            match restore.0 {
                Some(value) => env::set_var("NO_COLOR", value),
                None => env::remove_var("NO_COLOR"),
            }
            match restore.1 {
                Some(value) => env::set_var("TERM", value),
                None => env::remove_var("TERM"),
            }
        }
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
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"git\"]\nstyle = { fg = \"puce\" }",
                "unknown colour",
            ),
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"git\"]\n[segment.git]\nstyle = { glow = true }",
                "unknown style key",
            ),
        ];
        for (text, expected) in cases {
            let error = config::parse(text, None).expect_err(&format!("{text:?} should fail"));
            assert!(
                error.to_lowercase().contains(&expected.to_lowercase()),
                "{text:?} gave {error:?}, wanted {expected:?}"
            );
        }
    }

    /// A grid strip that is clipped still looks like a grid while hiding
    /// whatever fell off the end, so it must never be clipped at all. This is
    /// the flaw that the design document found in the text strip.
    fn atomic_config(fallback: bool) -> config::Config {
        let extra = if fallback {
            "fallback = [\"printf\", \"%s\", \"<2!>\"]\n"
        } else {
            ""
        };
        config::parse(
            &format!(
                r#"
views = ["g"]
[view.g]
label = "[env] "
segments = ["directory", "grid"]
[segment.grid]
command = ["printf", "%s", "ooo | o@o | oo!"]
atomic = true
{extra}"#
            ),
            None,
        )
        .expect("atomic config parses")
    }

    #[test]
    fn an_atomic_segment_is_shown_whole_or_not_at_all() {
        let config = atomic_config(false);
        // Wide enough: the grid is present and complete.
        let wide = render(&config, "g", 200, false).unwrap();
        assert!(wide.contains("ooo | o@o | oo!"), "{wide:?}");

        // Too narrow: the grid must vanish rather than appear truncated. A
        // partial grid is the one outcome worse than no grid.
        let narrow = render(&config, "g", 24, false).unwrap();
        assert!(!narrow.contains("ooo"), "a clipped grid survived: {narrow:?}");
    }

    #[test]
    fn an_atomic_segment_swaps_to_its_fallback_when_it_cannot_fit() {
        let config = atomic_config(true);
        let narrow = render(&config, "g", 24, false).unwrap();
        // The summary is honest about being a summary, and it keeps the alert.
        assert!(narrow.contains("<2!>"), "{narrow:?}");
        assert!(!narrow.contains("ooo"), "{narrow:?}");
    }

    #[test]
    fn a_shrinkable_segment_is_still_preferred_over_dropping_an_atomic_one() {
        // Sacrificing a whole segment is a bigger loss than trimming a path,
        // so the directory should give way first while the row still fits.
        let config = atomic_config(false);
        let row = render(&config, "g", 40, false).unwrap();
        assert!(row.contains("ooo | o@o | oo!"), "{row:?}");
    }

    #[test]
    fn a_fallback_that_also_does_not_fit_is_dropped_rather_than_looping() {
        // The degradation must terminate even when the fallback is itself too
        // wide, or layout would reconsider the same segment forever.
        let config = config::parse(
            r#"
views = ["g"]
[view.g]
label = "[env] "
segments = ["grid"]
[segment.grid]
command = ["printf", "%s", "ooo | o@o | oo!"]
atomic = true
fallback = ["printf", "%s", "still far too wide to fit in this row"]
"#,
            None,
        )
        .expect("parses");
        let row = render(&config, "g", 12, false).unwrap();
        assert!(!row.contains("still far too wide"), "{row:?}");
    }

    #[test]
    fn atomic_and_fallback_reject_contradictory_configuration() {
        let cases = [
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"s\"]\n[segment.s]\ncommand = [\"true\"]\nfallback = [\"true\"]",
                "needs atomic",
            ),
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"s\"]\n[segment.s]\ncommand = [\"true\"]\natomic = true\nshrink = true",
                "cannot set both",
            ),
            (
                "views = [\"a\"]\n[view.a]\nsegments = [\"s\"]\n[segment.s]\ncommand = [\"true\"]\natomic = true\nfallback = []",
                "must name a program",
            ),
        ];
        for (text, expected) in cases {
            let error = config::parse(text, None).expect_err(&format!("{text:?} should fail"));
            assert!(
                error.to_lowercase().contains(expected),
                "{text:?} gave {error:?}, wanted {expected:?}"
            );
        }
    }
}
