//! User configuration: which views exist, and what each one shows.
//!
//! Without a configuration file Whisker uses built-in defaults that match the
//! original prototype exactly, so an existing setup keeps working untouched.
use std::{collections::BTreeMap, env, fs, path::PathBuf};

use toml::Value;

use crate::style::Style;

/// The default configuration, also used as the `whisker config example` output.
pub const DEFAULT_CONFIG: &str = include_str!("default_config.toml");

/// Where a segment's text comes from.
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Source {
    /// Current directory, abbreviating the home directory to `~`.
    Directory,
    /// Branch and dirty marker, empty outside a repository.
    Git,
    /// Selected kubectl context and namespace.
    Kubernetes,
    /// A user-defined local command; argv, never a shell string.
    Command(Vec<String>),
}

#[derive(Clone, Debug)]
pub struct Segment {
    pub source: Source,
    /// Applied after layout, over the view's own style.
    pub style: Style,
    pub prefix: String,
    pub suffix: String,
    /// Segments that may be shortened when the row does not fit, longest first.
    pub shrink: bool,
    /// Shorten by dropping the beginning (`…/deep/tail`) rather than the end.
    pub keep_end: bool,
    /// Never show this segment partially: it is drawn whole or dropped.
    ///
    /// Shortening is safe for a path, where `…/tail` still reads truthfully,
    /// but not for a value whose meaning depends on being complete. A grid of
    /// status markers clipped mid-way still looks like a grid while hiding
    /// whatever fell off the end, which is worse than showing nothing.
    pub atomic: bool,
    /// What to show instead when an `atomic` segment does not fit.
    ///
    /// Empty means drop the segment entirely. A shorter summary is usually
    /// better than silence, so long as it is honest about being a summary.
    pub fallback: Option<Source>,
}

#[derive(Clone, Debug)]
pub struct ViewDef {
    pub name: String,
    pub label: String,
    pub label_style: Style,
    pub separator: String,
    pub separator_style: Style,
    pub segments: Vec<Segment>,
    /// Position on the optional 2D grid, as `[row, column]` indices into
    /// `Grid::rows` and `Grid::columns`.
    ///
    /// Views without a position are reachable by `view next` but not by
    /// directional movement, so an existing flat configuration is unaffected.
    pub at: Option<(usize, usize)>,
    /// The check that decides this node's state, run by `whisker collect`.
    pub alert: Option<Alert>,
}

/// The optional 2D arrangement of views.
///
/// The grid is a way of *navigating* the views that already exist rather than
/// a second kind of thing: a node is a view with coordinates.
#[derive(Clone, Debug, Default)]
pub struct Grid {
    pub rows: Vec<String>,
    pub columns: Vec<String>,
    /// Where per-view state is read from, if configured.
    ///
    /// Collecting each node's status inline would not scale: one Kubernetes
    /// check takes about 29 ms, so a nine-node grid would add a quarter of a
    /// second to every prompt before touching a network. Something else writes
    /// this file on its own schedule and the prompt only reads it, which costs
    /// microseconds and cannot block.
    pub state: Option<PathBuf>,
}

impl Grid {
    pub fn is_empty(&self) -> bool {
        self.rows.is_empty() || self.columns.is_empty()
    }
}

/// What a node is currently reporting.
///
/// Deliberately few: an alert either wants attention or it does not, and more
/// levels would need more colours than a dot can carry legibly.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum State {
    /// Nothing is known about this node, which is the honest default.
    #[default]
    Unknown,
    Ok,
    /// Something wants the user's attention.
    Alert,
}

impl State {
    fn parse(word: &str) -> Option<Self> {
        match word {
            "ok" => Some(State::Ok),
            "alert" => Some(State::Alert),
            "unknown" => Some(State::Unknown),
            _ => None,
        }
    }
}

/// What makes a node's check count as an alert.
///
/// Both reuse the existing `command` mechanism, so there is no new trust
/// boundary and no new evaluator: an alert is the same kind of thing a segment
/// already is, a local program run with argv.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum When {
    /// The command printed something. Suits a check that lists problems and
    /// says nothing when there are none.
    Output,
    /// The command exited non-zero. Suits a check written as an assertion.
    Exit,
}

impl When {
    fn parse(text: &str, what: &str) -> Result<Self, String> {
        match text {
            "output" => Ok(When::Output),
            "exit" => Ok(When::Exit),
            other => Err(format!(
                "{what}.when is {other}; use output or exit"
            )),
        }
    }
}

/// A node's check: what to run, and what counts as an alert.
#[derive(Clone, Debug)]
pub struct Alert {
    pub command: Vec<String>,
    pub when: When,
}

/// A direction to move in, from a directional key.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Direction {
    Left,
    Right,
    Up,
    Down,
}

impl Direction {
    pub fn parse(text: &str) -> Result<Self, String> {
        match text {
            "left" => Ok(Direction::Left),
            "right" => Ok(Direction::Right),
            "up" => Ok(Direction::Up),
            "down" => Ok(Direction::Down),
            other => Err(format!(
                "unknown direction: {other}; use left, right, up, or down"
            )),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Config {
    pub views: Vec<ViewDef>,
    pub start: String,
    /// The optional 2D arrangement; empty when no grid is configured.
    pub grid: Grid,
    /// The file the configuration came from, or None for built-in defaults.
    pub origin: Option<PathBuf>,
}

impl Config {
    pub fn view(&self, name: &str) -> Result<&ViewDef, String> {
        self.views
            .iter()
            .find(|view| view.name == name)
            .ok_or_else(|| format!("unknown view: {name}; use {}", self.view_names()))
    }

    pub fn view_names(&self) -> String {
        self.views
            .iter()
            .map(|view| view.name.as_str())
            .collect::<Vec<_>>()
            .join(", ")
    }

    /// The next view in the configured cycle, wrapping at the end.
    pub fn next(&self, current: &str) -> Result<&str, String> {
        let index = self
            .views
            .iter()
            .position(|view| view.name == current)
            .ok_or_else(|| format!("unknown view: {current}; use {}", self.view_names()))?;
        Ok(&self.views[(index + 1) % self.views.len()].name)
    }

    /// The view reached by moving one step from `current`, or `current` itself
    /// when there is nowhere to go.
    ///
    /// Two decisions are worth stating, because both are about not surprising
    /// the user rather than about convenience:
    ///
    /// Movement does not wrap. A grid is a map, and on a map moving left at
    /// the left edge does nothing; wrapping would teleport you across the
    /// screen for a keypress that felt like a nudge.
    ///
    /// Undefined cells are skipped rather than landed on. A sparse grid is
    /// common, since not every concern exists in every environment, and a key
    /// that appears to do nothing is worse than one that moves further than
    /// expected. The search continues in the same direction until it finds a
    /// defined view or runs off the edge.
    pub fn moved(&self, current: &str, direction: Direction) -> Result<&str, String> {
        let from = self
            .views
            .iter()
            .find(|view| view.name == current)
            .ok_or_else(|| format!("unknown view: {current}; use {}", self.view_names()))?;
        let Some((row, column)) = from.at else {
            // Without coordinates there is no direction to move in. Staying
            // put is the honest answer, and it keeps the key harmless in a
            // configuration that has no grid at all.
            return Ok(&from.name);
        };

        let (dr, dc) = match direction {
            Direction::Left => (0isize, -1isize),
            Direction::Right => (0, 1),
            Direction::Up => (-1, 0),
            Direction::Down => (1, 0),
        };
        let (mut r, mut c) = (row as isize, column as isize);
        loop {
            r += dr;
            c += dc;
            if r < 0
                || c < 0
                || r as usize >= self.grid.rows.len()
                || c as usize >= self.grid.columns.len()
            {
                return Ok(&from.name);
            }
            if let Some(view) = self
                .views
                .iter()
                .find(|view| view.at == Some((r as usize, c as usize)))
            {
                return Ok(&view.name);
            }
        }
    }

    /// Each view's current state, read from the configured state file.
    ///
    /// The file is untrusted: anything on the machine can write it, and a
    /// collector may be halfway through rewriting it when the prompt reads. So
    /// nothing here can fail. An unreadable file, a malformed line, an unknown
    /// view name, or an unknown status word all leave the affected node
    /// `Unknown` rather than raising an error, because a prompt that refuses
    /// to draw is worse than one that admits it does not know.
    ///
    /// Format is one line per view, `NAME STATUS [anything else]`:
    ///
    /// ```text
    /// infra_prod alert 2026-09-06T22:10:05 3 warning events
    /// infra_ops  ok
    /// ```
    ///
    /// Trailing words are ignored here, so a collector can record a timestamp
    /// and a human-readable reason in the same line.
    pub fn states(&self) -> Vec<State> {
        let mut states = vec![State::Unknown; self.views.len()];
        let Some(path) = &self.grid.state else {
            return states;
        };
        let Ok(text) = fs::read_to_string(path) else {
            return states;
        };
        for line in text.lines() {
            let mut words = line.split_whitespace();
            let (Some(name), Some(status)) = (words.next(), words.next()) else {
                continue;
            };
            let Some(state) = State::parse(status) else {
                continue;
            };
            if let Some(index) = self.views.iter().position(|view| view.name == name) {
                states[index] = state;
            }
        }
        states
    }

    /// The grid as one line per row, each cell being a view name or `-` for an
    /// undefined cell.
    ///
    /// Emitting the shape rather than a drawn picture keeps the layout
    /// decision, text strip or image, outside this function, and lets a shell
    /// script render it however it likes.
    pub fn grid_rows(&self) -> Vec<String> {
        if self.grid.is_empty() {
            return Vec::new();
        }
        let states = self.states();
        self.grid
            .rows
            .iter()
            .enumerate()
            .map(|(r, _)| {
                let cells: Vec<String> = (0..self.grid.columns.len())
                    .map(|c| {
                        match self
                            .views
                            .iter()
                            .position(|view| view.at == Some((r, c)))
                        {
                            // Name and state together, so a reader never has
                            // to correlate two separate listings and risk
                            // pairing a state with the wrong node.
                            Some(index) => format!(
                                "{}:{}",
                                self.views[index].name,
                                match states[index] {
                                    State::Ok => "ok",
                                    State::Alert => "alert",
                                    State::Unknown => "unknown",
                                }
                            ),
                            None => "-".to_string(),
                        }
                    })
                    .collect();
                cells.join(" ")
            })
            .collect()
    }
}

/// The configuration file path: `$WHISKER_CONFIG`, else
/// `$XDG_CONFIG_HOME/whisker/config.toml`, else `~/.config/whisker/config.toml`.
pub fn path() -> Option<PathBuf> {
    if let Some(explicit) = env::var_os("WHISKER_CONFIG") {
        return Some(PathBuf::from(explicit));
    }
    let base = env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|path| path.is_absolute())
        .or_else(|| env::var_os("HOME").map(|home| PathBuf::from(home).join(".config")))?;
    Some(base.join("whisker").join("config.toml"))
}

/// Load the user's configuration, falling back to the built-in defaults when no
/// file exists. A file that exists but cannot be read or parsed is an error, so
/// a typo is reported rather than silently ignored.
pub fn load() -> Result<Config, String> {
    let Some(file) = path() else {
        return parse(DEFAULT_CONFIG, None);
    };
    match fs::read_to_string(&file) {
        Ok(text) => parse(&text, Some(file)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => parse(DEFAULT_CONFIG, None),
        Err(error) => Err(format!("cannot read {}: {error}", file.display())),
    }
}

fn as_str(value: &Value, what: &str) -> Result<String, String> {
    value
        .as_str()
        .map(str::to_owned)
        .ok_or_else(|| format!("{what} must be a string"))
}

fn as_bool(value: &Value, what: &str) -> Result<bool, String> {
    value
        .as_bool()
        .ok_or_else(|| format!("{what} must be true or false"))
}

fn known_keys(table: &toml::Table, allowed: &[&str], what: &str) -> Result<(), String> {
    for key in table.keys() {
        if !allowed.contains(&key.as_str()) {
            return Err(format!(
                "unknown key {key} in {what}; use {}",
                allowed.join(", ")
            ));
        }
    }
    Ok(())
}

fn builtin(name: &str) -> Option<Source> {
    match name {
        "directory" => Some(Source::Directory),
        "git" => Some(Source::Git),
        "kubernetes" => Some(Source::Kubernetes),
        _ => None,
    }
}

fn segment(name: &str, table: Option<&toml::Table>) -> Result<Segment, String> {
    let what = format!("segment.{name}");
    let mut segment = Segment {
        source: builtin(name).unwrap_or(Source::Command(Vec::new())),
        style: Style::default(),
        prefix: String::new(),
        suffix: String::new(),
        // The directory is the one long, safely truncatable segment by default.
        shrink: name == "directory",
        keep_end: name == "directory",
        atomic: false,
        fallback: None,
    };
    let Some(table) = table else {
        return builtin(name)
            .map(|_| segment)
            .ok_or_else(|| format!("unknown segment: {name}; define [segment.{name}] first"));
    };
    known_keys(
        table,
        &[
            "command", "prefix", "suffix", "shrink", "keep_end", "style",
            "atomic", "fallback",
        ],
        &what,
    )?;
    if let Some(command) = table.get("command") {
        if builtin(name).is_some() {
            return Err(format!("{what} is built in and cannot set command"));
        }
        let argv = command
            .as_array()
            .ok_or_else(|| format!("{what}.command must be an array of strings"))?
            .iter()
            .map(|item| as_str(item, &format!("{what}.command entry")))
            .collect::<Result<Vec<_>, _>>()?;
        if argv.is_empty() {
            return Err(format!("{what}.command must name a program"));
        }
        segment.shrink = true;
        segment.source = Source::Command(argv);
    } else if builtin(name).is_none() {
        return Err(format!("{what} must set command"));
    }
    if let Some(value) = table.get("prefix") {
        segment.prefix = as_str(value, &format!("{what}.prefix"))?;
    }
    if let Some(value) = table.get("suffix") {
        segment.suffix = as_str(value, &format!("{what}.suffix"))?;
    }
    if let Some(value) = table.get("shrink") {
        segment.shrink = as_bool(value, &format!("{what}.shrink"))?;
    }
    if let Some(value) = table.get("keep_end") {
        segment.keep_end = as_bool(value, &format!("{what}.keep_end"))?;
    }
    if let Some(value) = table.get("style") {
        segment.style = Style::parse(value, &format!("{what}.style"))?;
    }
    if let Some(value) = table.get("atomic") {
        segment.atomic = as_bool(value, &format!("{what}.atomic"))?;
    }
    if let Some(value) = table.get("fallback") {
        let argv = value
            .as_array()
            .ok_or_else(|| format!("{what}.fallback must be an array of strings"))?
            .iter()
            .map(|item| as_str(item, &format!("{what}.fallback entry")))
            .collect::<Result<Vec<_>, _>>()?;
        if argv.is_empty() {
            return Err(format!("{what}.fallback must name a program"));
        }
        segment.fallback = Some(Source::Command(argv));
    }
    // A fallback only ever applies when the segment refuses to be clipped, so
    // configuring one without `atomic` is a mistake worth naming rather than
    // quietly ignoring.
    if segment.fallback.is_some() && !segment.atomic {
        return Err(format!("{what}.fallback needs atomic = true to take effect"));
    }
    // Shrinking and atomicity are contradictory instructions: one says clip me,
    // the other says never show me clipped.
    if segment.atomic && segment.shrink && table.get("shrink").is_some() {
        return Err(format!("{what} cannot set both atomic and shrink"));
    }
    if segment.atomic {
        segment.shrink = false;
    }
    Ok(segment)
}

pub fn parse(text: &str, origin: Option<PathBuf>) -> Result<Config, String> {
    let root: toml::Table = text.parse().map_err(|error| format!("{error}"))?;
    known_keys(
        &root,
        &["views", "start", "view", "segment", "grid"],
        "configuration",
    )?;

    // The grid is optional; without it Whisker behaves exactly as before.
    let mut grid = Grid::default();
    if let Some(value) = root.get("grid") {
        let table = value
            .as_table()
            .ok_or_else(|| "grid must be a table".to_string())?;
        known_keys(table, &["rows", "columns", "state"], "grid")?;
        let axis = |key: &str| -> Result<Vec<String>, String> {
            table
                .get(key)
                .ok_or_else(|| format!("grid must set {key}"))?
                .as_array()
                .ok_or_else(|| format!("grid.{key} must be an array of names"))?
                .iter()
                .map(|item| as_str(item, &format!("grid.{key} entry")))
                .collect()
        };
        grid.rows = axis("rows")?;
        grid.columns = axis("columns")?;
        if grid.rows.is_empty() || grid.columns.is_empty() {
            return Err("grid.rows and grid.columns must each name an axis".into());
        }
        if let Some(value) = table.get("state") {
            let text = as_str(value, "grid.state")?;
            // The file is read at prompt time, not now: it may not exist yet
            // when the shell starts, and a collector may create it later.
            grid.state = Some(PathBuf::from(text));
        }
    }

    let mut segments: BTreeMap<String, Segment> = BTreeMap::new();
    if let Some(defined) = root.get("segment") {
        let defined = defined
            .as_table()
            .ok_or("segment must be a table of [segment.name] entries")?;
        for (name, value) in defined {
            let table = value
                .as_table()
                .ok_or_else(|| format!("segment.{name} must be a table"))?;
            segments.insert(name.clone(), segment(name, Some(table))?);
        }
    }

    let defined_views = root
        .get("view")
        .map(|value| {
            value
                .as_table()
                .ok_or("view must be a table of [view.name] entries")
        })
        .transpose()?;

    let order: Vec<String> = match root.get("views") {
        Some(value) => value
            .as_array()
            .ok_or("views must be an array of view names")?
            .iter()
            .map(|item| as_str(item, "views entry"))
            .collect::<Result<_, _>>()?,
        // Without an explicit cycle, use the defined views in file order.
        None => defined_views
            .map(|table| table.keys().cloned().collect())
            .unwrap_or_default(),
    };
    if order.is_empty() {
        return Err("configure at least one view".into());
    }

    let mut views = Vec::new();
    for name in &order {
        if views.iter().any(|view: &ViewDef| &view.name == name) {
            return Err(format!("view {name} is listed twice in views"));
        }
        let table = defined_views
            .and_then(|table| table.get(name))
            .ok_or_else(|| format!("views lists {name} but [view.{name}] is missing"))?
            .as_table()
            .ok_or_else(|| format!("view.{name} must be a table"))?;
        known_keys(
            table,
            &[
                "label",
                "separator",
                "segments",
                "style",
                "label_style",
                "separator_style",
                "at",
                "alert",
            ],
            &format!("view.{name}"),
        )?;
        // A view's grid position names its row and column, so a configuration
        // reads as coordinates rather than as indices to be counted out.
        let at = match table.get("at") {
            None => None,
            Some(value) => {
                if grid.is_empty() {
                    return Err(format!("view.{name}.at needs a [grid] to sit on"));
                }
                let pair = value
                    .as_array()
                    .filter(|array| array.len() == 2)
                    .ok_or_else(|| format!("view.{name}.at must be [row, column]"))?;
                let row_name = as_str(&pair[0], &format!("view.{name}.at row"))?;
                let column_name = as_str(&pair[1], &format!("view.{name}.at column"))?;
                let row = grid.rows.iter().position(|r| *r == row_name).ok_or_else(|| {
                    format!(
                        "view.{name}.at names row {row_name}, which is not in grid.rows"
                    )
                })?;
                let column = grid
                    .columns
                    .iter()
                    .position(|c| *c == column_name)
                    .ok_or_else(|| {
                        format!(
                            "view.{name}.at names column {column_name}, \
                             which is not in grid.columns"
                        )
                    })?;
                Some((row, column))
            }
        };
        let view_style = table
            .get("style")
            .map(|value| Style::parse(value, &format!("view.{name}.style")))
            .transpose()?
            .unwrap_or_default();
        // A label with no style of its own inherits the view's.
        let label_style = table
            .get("label_style")
            .map(|value| Style::parse(value, &format!("view.{name}.label_style")))
            .transpose()?
            .unwrap_or(view_style);
        let separator_style = table
            .get("separator_style")
            .map(|value| Style::parse(value, &format!("view.{name}.separator_style")))
            .transpose()?
            .unwrap_or_default();
        let alert = match table.get("alert") {
            None => None,
            Some(value) => {
                let what = format!("view.{name}.alert");
                let alert_table = value
                    .as_table()
                    .ok_or_else(|| format!("{what} must be a table"))?;
                known_keys(alert_table, &["command", "when"], &what)?;
                let command: Vec<String> = alert_table
                    .get("command")
                    .ok_or_else(|| format!("{what} must set command"))?
                    .as_array()
                    .ok_or_else(|| format!("{what}.command must be an array of strings"))?
                    .iter()
                    .map(|item| as_str(item, &format!("{what}.command entry")))
                    .collect::<Result<_, _>>()?;
                if command.is_empty() {
                    return Err(format!("{what}.command must name a program"));
                }
                // `when` has no safe default: "printed something" and "exited
                // non-zero" disagree for most commands, and guessing wrong
                // means either constant alerts or none at all.
                let when = When::parse(
                    &as_str(
                        alert_table
                            .get("when")
                            .ok_or_else(|| format!("{what} must set when"))?,
                        &format!("{what}.when"),
                    )?,
                    &what,
                )?;
                Some(Alert { command, when })
            }
        };

        let names: Vec<String> = table
            .get("segments")
            .ok_or_else(|| format!("view.{name} must set segments"))?
            .as_array()
            .ok_or_else(|| format!("view.{name}.segments must be an array of segment names"))?
            .iter()
            .map(|item| as_str(item, &format!("view.{name}.segments entry")))
            .collect::<Result<_, _>>()?;
        if names.is_empty() {
            return Err(format!("view.{name}.segments must name a segment"));
        }
        views.push(ViewDef {
            name: name.clone(),
            label: table
                .get("label")
                .map(|value| as_str(value, &format!("view.{name}.label")))
                .transpose()?
                .unwrap_or_default(),
            separator: table
                .get("separator")
                .map(|value| as_str(value, &format!("view.{name}.separator")))
                .transpose()?
                .unwrap_or_else(|| "  ".into()),
            label_style,
            separator_style,
            at,
            alert,
            segments: names
                .iter()
                .map(|segment_name| {
                    let mut found = match segments.get(segment_name) {
                        Some(found) => found.clone(),
                        None => segment(segment_name, None)
                            .map_err(|error| format!("view.{name}: {error}"))?,
                    };
                    // The segment's own style wins over the view-wide default.
                    found.style = found.style.or(view_style);
                    Ok::<Segment, String>(found)
                })
                .collect::<Result<_, _>>()?,
        });
    }

    let start = match root.get("start") {
        Some(value) => as_str(value, "start")?,
        None => views[0].name.clone(),
    };
    if !views.iter().any(|view| view.name == start) {
        return Err(format!(
            "start names {start}, which is not a configured view"
        ));
    }

    // Two views on one cell would make movement ambiguous: a direction key
    // could land on either, and which one you got would depend on ordering.
    for (index, view) in views.iter().enumerate() {
        if let Some(at) = view.at {
            if let Some(other) = views[..index].iter().find(|other| other.at == Some(at)) {
                return Err(format!(
                    "view.{} and view.{} both sit at [{}, {}]",
                    other.name, view.name, grid.rows[at.0], grid.columns[at.1]
                ));
            }
        }
    }

    Ok(Config {
        views,
        start,
        grid,
        origin,
    })
}
