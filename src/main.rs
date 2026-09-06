//! PROTOTYPE: collect one information row; Bash owns the editing session.
use std::{
    env,
    path::Path,
    process::{self, Command, Stdio},
};
use unicode_width::UnicodeWidthChar;

#[derive(Clone, Copy)]
enum View {
    Minimal,
    Dev,
    Ops,
}

impl View {
    fn parse(value: &str) -> Result<Self, String> {
        match value {
            "minimal" => Ok(Self::Minimal),
            "dev" => Ok(Self::Dev),
            "ops" => Ok(Self::Ops),
            _ => Err(format!("unknown view: {value}; use minimal, dev, or ops")),
        }
    }

    fn next(self) -> &'static str {
        match self {
            Self::Minimal => "dev",
            Self::Dev => "ops",
            Self::Ops => "minimal",
        }
    }
}

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

fn layout(view: View, directory: &str, detail: &str, columns: usize) -> String {
    // Leave one column spare to avoid automatic wrapping on the info row.
    let budget = columns.saturating_sub(1);
    let path = clean(directory);
    if matches!(view, View::Minimal) {
        return clip(&path, budget, true);
    }
    let label = if matches!(view, View::Dev) {
        "[dev] "
    } else {
        "[ops] "
    };
    let detail = clean(detail);
    let suffix = if detail.is_empty() {
        String::new()
    } else {
        format!("  {detail}")
    };
    let available = budget.saturating_sub(width(label) + width(&suffix));
    // Shorten the directory first. On very narrow terminals cap the whole row.
    let path = clip(&path, available.max(1), true);
    clip(&format!("{label}{path}{suffix}"), budget, false)
}

fn run() -> Result<(), String> {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() || matches!(args[0].as_str(), "--help" | "-h") {
        println!(
            "Whisker PROTOTYPE\n\n  whisker render [--view minimal|dev|ops] [--columns N]\n  whisker view next --current minimal|dev|ops\n\nRun ./try-it for the interactive Bash experiment."
        );
        return Ok(());
    }
    if args.len() == 4 && args[..3] == ["view", "next", "--current"] {
        println!("{}", View::parse(&args[3])?.next());
        return Ok(());
    }
    if args[0] != "render" {
        return Err("expected render or view next; see --help".into());
    }
    let mut view = View::Dev;
    let mut columns = 80;
    let mut options = args[1..].iter();
    while let Some(option) = options.next() {
        let value = options
            .next()
            .ok_or_else(|| format!("missing value for {option}"))?;
        match option.as_str() {
            "--view" => view = View::parse(value)?,
            "--columns" => {
                columns = value
                    .parse::<usize>()
                    .map_err(|_| "columns must be an integer")?
                    .clamp(1, 16384)
            }
            _ => return Err(format!("unknown option: {option}")),
        }
    }
    let detail = match view {
        View::Minimal => String::new(),
        View::Dev => git().unwrap_or_default(),
        View::Ops => kubernetes(),
    };
    println!("{}", layout(view, &directory(), &detail, columns));
    Ok(())
}

fn main() {
    if let Err(error) = run() {
        eprintln!("whisker: {error}");
        process::exit(2);
    }
}
