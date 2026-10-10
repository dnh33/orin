//! `orin tui` — an interactive fuzzy picker over the daemon's resident index.
//!
//! Typing re-queries the running daemon (auto-spawned exactly the way
//! `orin query` spawns it) over the named-pipe protocol with the same query
//! language. Searches run on a worker thread behind a short debounce, so
//! keystrokes never wait on the pipe. The picker draws to stderr and writes
//! the chosen path to stdout only after the terminal is restored, which
//! keeps `PATH=$(orin tui)` free of escape codes.

use super::client;
use anyhow::Context;
use crossterm::cursor::Show;
use crossterm::event::Event;
use crossterm::event::KeyCode;
use crossterm::event::KeyEvent;
use crossterm::event::KeyEventKind;
use crossterm::event::KeyModifiers;
use crossterm::event::poll;
use crossterm::event::read;
use crossterm::terminal::EnterAlternateScreen;
use crossterm::terminal::LeaveAlternateScreen;
use crossterm::terminal::disable_raw_mode;
use crossterm::terminal::enable_raw_mode;
use orin_core::protocol::HitWire;
use ratatui::Frame;
use ratatui::Terminal;
use ratatui::backend::CrosstermBackend;
use ratatui::layout::Constraint;
use ratatui::layout::Layout;
use ratatui::layout::Position;
use ratatui::layout::Rect;
use ratatui::style::Color;
use ratatui::style::Modifier;
use ratatui::style::Style;
use ratatui::text::Line;
use ratatui::text::Span;
use ratatui::widgets::List;
use ratatui::widgets::ListItem;
use ratatui::widgets::ListState;
use ratatui::widgets::Paragraph;
use std::io;
use std::io::Stderr;
use std::panic;
use std::process::ExitCode;
use std::sync::mpsc;
use std::time::Duration;
use std::time::Instant;

/// Debounce: one daemon query per pause in typing, never one per keystroke.
const DEBOUNCE: Duration = Duration::from_millis(40);
/// Poll interval while a search runs or a query waits to go out.
const TICK: Duration = Duration::from_millis(10);
/// Idle poll interval, so a quiet picker costs almost nothing.
const IDLE: Duration = Duration::from_millis(50);
/// Rows requested per search: plenty for a picker, cheap to render.
const QUERY_LIMIT: u32 = 1_000;
/// Exit status when the selection is abandoned (Esc, `q`, Ctrl-C).
const CANCELLED: u8 = 1;
/// Footer: the whole key map on one dim line.
const HELP: &str = "type to search · Up/Down or Ctrl-P/Ctrl-N navigate · Enter select · Esc/q quit";

/// The picker's terminal: stderr-backed so stdout carries only the path.
type Term = Terminal<CrosstermBackend<Stderr>>;

/// One accent, 256-color-safe: bold yellow on the matched slice of a path.
fn accent() -> Style {
    Style::new().fg(Color::Yellow).add_modifier(Modifier::BOLD)
}

/// Dim gray chrome: prompt, status and help are context, not content.
fn chrome() -> Style {
    Style::new().fg(Color::DarkGray)
}

/// Status-line color for daemon failures.
fn failure() -> Style {
    Style::new().fg(Color::Red)
}

/// Work handed to the background search worker.
enum Job {
    /// Touch the daemon once (auto-spawn included) before typing starts.
    Wake,
    /// One search, worded exactly as `orin query` would word it.
    Search(String),
}

/// Worker replies, delivered in job order.
enum Outcome {
    /// Wake-up touch; `Err` means the daemon could not be reached.
    Wake(anyhow::Result<()>),
    /// Search round-trip time plus hits, or the daemon error that replaced them.
    Search(Duration, anyhow::Result<Vec<HitWire>>),
}

/// What the event loop hands back to `run`.
enum Pick {
    /// Enter was pressed on a row.
    Chosen(String),
    /// Esc, `q` on an empty query, or Ctrl-C: leave without selecting.
    Cancelled,
}

/// One key press, reduced to what the picker must do about it.
enum Action {
    /// Not a key we act on: releases, modifiers, function keys.
    Ignore,
    /// Abandon the selection and exit non-zero.
    Quit,
    /// Enter: take the selected row.
    Select,
    /// Up/Down and Ctrl-P/Ctrl-N: signed row delta.
    Move(isize),
    /// Backspace: drop the last query character.
    Delete,
    /// A printable character for the query.
    Insert(char),
}

/// `orin tui`: draw the picker, then print (or open) the chosen path.
pub(crate) fn run(open: bool) -> anyhow::Result<ExitCode> {
    let mut terminal = init_terminal()?;
    let guard = Restore;
    let picked = event_loop(&mut terminal);
    // Restore before anything touches stdout, so pipelines capture only the
    // selected path and never an escape sequence.
    drop(guard);
    match picked? {
        Pick::Chosen(path) => {
            if open {
                open::that(&path).with_context(|| format!("failed to open {path}"))?;
            } else {
                println!("{path}");
            }
            Ok(ExitCode::SUCCESS)
        }
        Pick::Cancelled => Ok(ExitCode::from(CANCELLED)),
    }
}

/// Map a key event to an action; `empty_query` decides whether `q` quits.
fn action_for(key: &KeyEvent, empty_query: bool) -> Action {
    if !matches!(key.kind, KeyEventKind::Press | KeyEventKind::Repeat) {
        return Action::Ignore;
    }
    let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
    let alt = key.modifiers.contains(KeyModifiers::ALT);
    match key.code {
        KeyCode::Esc => Action::Quit,
        KeyCode::Enter => Action::Select,
        KeyCode::Up => Action::Move(-1),
        KeyCode::Down => Action::Move(1),
        KeyCode::Backspace => Action::Delete,
        KeyCode::Char('c') if ctrl => Action::Quit,
        KeyCode::Char('n') if ctrl => Action::Move(1),
        KeyCode::Char('p') if ctrl => Action::Move(-1),
        // `q` types itself as soon as there is a query to extend.
        KeyCode::Char('q') if empty_query && !ctrl && !alt => Action::Quit,
        KeyCode::Char(ch) if !ctrl && !alt => Action::Insert(ch),
        _ => Action::Ignore,
    }
}

/// Mutable picker state; only the main thread ever touches it.
struct App {
    /// Query text as typed, handed to the daemon verbatim.
    query: String,
    /// Newest completed result set, best match first.
    hits: Vec<HitWire>,
    /// Selection index into `hits`, and the scroll offset behind it.
    state: ListState,
    /// Deadline of the query that is waiting to go out, if any.
    debounce_at: Option<Instant>,
    /// A job is on the worker, so no other job may be dispatched.
    busy: bool,
    /// Round-trip time of the last completed search, in milliseconds.
    last_ms: u128,
    /// Latest daemon failure, cleared by the next success.
    error: Option<String>,
    /// Set whenever state changes so the loop can skip redundant draws.
    redraw: bool,
}

impl App {
    fn new() -> Self {
        Self {
            query: String::new(),
            hits: Vec::new(),
            state: ListState::default(),
            debounce_at: None,
            busy: true,
            last_ms: 0,
            error: None,
            redraw: true,
        }
    }

    /// True while a search is running or one is about to be sent.
    fn pending(&self) -> bool {
        self.busy || self.debounce_at.is_some()
    }

    /// Replace the query, arming the debounce — or clearing the empty state.
    fn set_query(&mut self, next: String) {
        if next == self.query {
            return;
        }
        self.query = next;
        self.redraw = true;
        if self.query.trim().is_empty() {
            self.hits.clear();
            self.state.select(None);
            self.debounce_at = None;
            return;
        }
        self.debounce_at = Some(Instant::now() + DEBOUNCE);
    }

    fn insert_char(&mut self, ch: char) {
        let mut next = self.query.clone();
        next.push(ch);
        self.set_query(next);
    }

    fn delete_char(&mut self) {
        let mut next = self.query.clone();
        let _ = next.pop();
        self.set_query(next);
    }

    /// Move the selection by `delta` rows, clamped to the result list.
    fn move_by(&mut self, delta: isize) {
        if self.hits.is_empty() {
            return;
        }
        let last = self.hits.len() - 1;
        let current = self.state.selected().unwrap_or(0).min(last);
        let step = delta.unsigned_abs();
        let next = if delta < 0 {
            current.saturating_sub(step)
        } else {
            current.saturating_add(step).min(last)
        };
        self.state.select(Some(next));
        self.redraw = true;
    }

    /// Path of the selected hit, if a row is selected.
    fn selected_path(&self) -> Option<String> {
        self.state
            .selected()
            .and_then(|index| self.hits.get(index))
            .map(|hit| hit.p.clone())
    }

    /// A query may go out once typing has paused and the worker is free.
    fn should_dispatch(&self, now: Instant) -> bool {
        !self.busy && self.debounce_at.is_some_and(|at| now >= at) && !self.query.trim().is_empty()
    }

    /// Hand the current query to the worker; exactly one runs at a time.
    fn dispatch(&mut self, jobs: &mpsc::Sender<Job>) {
        self.debounce_at = None;
        self.busy = true;
        self.redraw = true;
        let job = Job::Search(self.query.clone());
        if jobs.send(job).is_err() {
            // The worker is gone: stop dispatching, but stay on screen.
            self.error = Some("search worker stopped".to_string());
        }
    }

    /// Fold a worker reply into the state; replies arrive one at a time.
    fn absorb(&mut self, outcome: Outcome) {
        match outcome {
            Outcome::Wake(result) => {
                self.busy = false;
                self.redraw = true;
                self.error = result.err().map(|err| err.to_string());
            }
            Outcome::Search(took, result) => {
                self.busy = false;
                self.redraw = true;
                self.last_ms = took.as_millis();
                match result {
                    Ok(hits) => {
                        self.error = None;
                        // Drop results for a query the user already cleared.
                        if !self.query.trim().is_empty() {
                            self.hits = hits;
                            let first = (!self.hits.is_empty()).then_some(0);
                            self.state.select(first);
                        }
                    }
                    Err(err) => self.error = Some(err.to_string()),
                }
            }
        }
    }

    /// How long the loop may sleep before it must look at state again.
    fn timeout(&self, now: Instant) -> Duration {
        if let Some(at) = self.debounce_at {
            // Cap the wait so an early reply is noticed promptly; a spent
            // deadline means the worker is busy, which polls at `TICK`.
            let left = at.saturating_duration_since(now);
            if left.is_zero() { TICK } else { left.min(TICK) }
        } else if self.busy {
            TICK
        } else {
            IDLE
        }
    }
}

/// Start the search worker; the UI thread never touches the pipe itself.
fn spawn_worker() -> (mpsc::Sender<Job>, mpsc::Receiver<Outcome>) {
    let (jobs_tx, jobs_rx) = mpsc::channel::<Job>();
    let (out_tx, out_rx) = mpsc::channel::<Outcome>();
    // Dropping the handle detaches the worker: the picker never joins it.
    let _worker = std::thread::spawn(move || {
        while let Ok(job) = jobs_rx.recv() {
            let outcome = match job {
                Job::Wake => Outcome::Wake(client::status().map(|_| ())),
                Job::Search(query) => {
                    let started = Instant::now();
                    let result = client::query(&query, QUERY_LIMIT);
                    Outcome::Search(started.elapsed(), result)
                }
            };
            if out_tx.send(outcome).is_err() {
                break;
            }
        }
    });
    (jobs_tx, out_rx)
}

/// Draw, wait, dispatch: the single-threaded half of the design.
fn event_loop(terminal: &mut Term) -> anyhow::Result<Pick> {
    let (jobs, outcomes) = spawn_worker();
    let mut app = App::new();
    // Wake the daemon up front so the first keystroke lands on a warm index.
    if jobs.send(Job::Wake).is_err() {
        app.error = Some("search worker stopped".to_string());
    }
    loop {
        while let Ok(outcome) = outcomes.try_recv() {
            app.absorb(outcome);
        }
        let now = Instant::now();
        if app.should_dispatch(now) {
            app.dispatch(&jobs);
        }
        if app.redraw {
            terminal.draw(|frame| draw(frame, &mut app))?;
        }
        if poll(app.timeout(now))? {
            match read()? {
                Event::Key(key) => match action_for(&key, app.query.is_empty()) {
                    Action::Ignore => {}
                    Action::Quit => return Ok(Pick::Cancelled),
                    Action::Select => {
                        if let Some(path) = app.selected_path() {
                            return Ok(Pick::Chosen(path));
                        }
                    }
                    Action::Move(delta) => app.move_by(delta),
                    Action::Delete => app.delete_char(),
                    Action::Insert(ch) => app.insert_char(ch),
                },
                // A resize only needs a redraw: ratatui re-measures on draw.
                Event::Resize(_, _) => app.redraw = true,
                _ => {}
            }
        }
    }
}

/// Render one frame: query line, results, status, help.
fn draw(frame: &mut Frame, app: &mut App) {
    let [input_area, list_area, status_area, help_area] = row_areas(frame.area());
    draw_query(frame, app, input_area);
    draw_results(frame, app, list_area);
    draw_status(frame, app, status_area);
    frame.render_widget(Paragraph::new(HELP).style(chrome()), help_area);
    app.redraw = false;
}

/// The query line: a dim prompt, the text as typed, and a cursor on it.
fn draw_query(frame: &mut Frame, app: &App, area: Rect) {
    let prompt = Span::styled("> ", chrome());
    let typed = Span::raw(app.query.as_str());
    frame.render_widget(Paragraph::new(Line::from(vec![prompt, typed])), area);
    if area.width > 0 {
        // One column per character: exact for ASCII, cosmetic for wide glyphs.
        let count = u16::try_from(app.query.chars().count()).unwrap_or(u16::MAX);
        let column = count.saturating_add(2).min(area.width - 1);
        frame.set_cursor_position(Position {
            x: area.x.saturating_add(column),
            y: area.y,
        });
    }
}

/// The result list, with the matched slice of each path in the accent.
fn draw_results(frame: &mut Frame, app: &mut App, area: Rect) {
    if app.hits.is_empty() {
        frame.render_widget(Paragraph::new(empty_hint(app)).style(chrome()), area);
    } else {
        let needles = query_needles(&app.query);
        let mut items: Vec<ListItem> = Vec::with_capacity(app.hits.len());
        for hit in &app.hits {
            let line = Line::from(path_spans(&hit.p, &needles));
            items.push(ListItem::new(line));
        }
        let selected = Style::new().add_modifier(Modifier::REVERSED);
        let list = List::new(items).highlight_style(selected);
        frame.render_stateful_widget(list, area, &mut app.state);
    }
}

/// Empty-state line: the required hint, or why there is nothing to show.
fn empty_hint(app: &App) -> &'static str {
    if app.query.trim().is_empty() {
        "type to search"
    } else if app.error.is_some() {
        "daemon unreachable - type to retry"
    } else {
        "no matches"
    }
}

/// Match count and query latency, or the daemon failure that replaced them.
fn draw_status(frame: &mut Frame, app: &App, area: Rect) {
    let (text, style) = match &app.error {
        Some(error) => (format!("error: {error}"), failure()),
        None => {
            let count = app.hits.len();
            let took = app.last_ms;
            let mut text = format!("{count} matches · {took}ms");
            if app.pending() {
                text.push_str(" · ...");
            }
            (text, chrome())
        }
    };
    frame.render_widget(Paragraph::new(text).style(style), area);
}

/// Four fixed rows: query, results, status, help.
fn row_areas(area: Rect) -> [Rect; 4] {
    let layout = Layout::vertical([
        Constraint::Length(1),
        Constraint::Min(1),
        Constraint::Length(1),
        Constraint::Length(1),
    ]);
    let rows = layout.split(area);
    [rows[0], rows[1], rows[2], rows[3]]
}

/// Literal substrings worth accenting for a query.
///
/// The daemon owns matching; this only picks which slices of a path light up.
/// Predicates that never appear in a path (`size:`, `type:`, `mtime:`, `re:`,
/// negations, globs) are skipped, while `path:`/`prefix:`/`ext:` keep their
/// value because it does show up in the result.
fn query_needles(query: &str) -> Vec<String> {
    tokenize(query)
        .into_iter()
        .flat_map(|token| needles_for_token(&token))
        .collect()
}

/// Split on whitespace, dropping the quote characters themselves.
fn tokenize(query: &str) -> Vec<String> {
    let mut tokens = Vec::new();
    let mut current = String::new();
    let mut quoted = false;
    for ch in query.chars() {
        match ch {
            '"' => quoted = !quoted,
            _ if ch.is_whitespace() && !quoted => {
                if !current.is_empty() {
                    tokens.push(std::mem::take(&mut current));
                }
            }
            _ => current.push(ch),
        }
    }
    if !current.is_empty() {
        tokens.push(current);
    }
    tokens
}

/// Needles contributed by one token of the query language.
fn needles_for_token(token: &str) -> Vec<String> {
    if token.starts_with('!') {
        return Vec::new();
    }
    match token.split_once(':') {
        Some(("path", rest)) | Some(("prefix", rest)) => single_needle(rest),
        Some(("ext", rest)) => rest
            .split(',')
            .filter(|ext| !ext.is_empty() && !ext.starts_with('!'))
            .map(|ext| {
                if ext.starts_with('.') {
                    ext.to_string()
                } else {
                    format!(".{ext}")
                }
            })
            .collect(),
        Some(_) => Vec::new(),
        None if token.contains('*') || token.contains('?') => Vec::new(),
        None => single_needle(token),
    }
}

fn single_needle(value: &str) -> Vec<String> {
    if value.is_empty() {
        Vec::new()
    } else {
        vec![value.to_string()]
    }
}

/// Split a path into plain and accent-styled spans around its matches.
fn path_spans(path: &str, needles: &[String]) -> Vec<Span<'static>> {
    let mut spans = Vec::new();
    let mut at = 0;
    for (start, end) in match_ranges(path, needles) {
        if start > at {
            spans.push(Span::raw(path[at..start].to_string()));
        }
        spans.push(Span::styled(path[start..end].to_string(), accent()));
        at = end;
    }
    if at < path.len() {
        spans.push(Span::raw(path[at..].to_string()));
    }
    spans
}

/// Case-insensitive, byte-wise match ranges, sorted and merged.
fn match_ranges(haystack: &str, needles: &[String]) -> Vec<(usize, usize)> {
    if needles.is_empty() || haystack.is_empty() {
        return Vec::new();
    }
    let folded = fold_ascii(haystack);
    let mut ranges = Vec::new();
    for needle in needles {
        if needle.is_empty() {
            continue;
        }
        let target = fold_ascii(needle);
        let mut from = 0;
        while let Some(offset) = folded[from..].find(target.as_str()) {
            let start = from + offset;
            ranges.push((start, start + target.len()));
            from = start + target.len();
        }
    }
    ranges.sort_unstable();
    let mut merged: Vec<(usize, usize)> = Vec::with_capacity(ranges.len());
    for (start, end) in ranges {
        // Read first, then extend by index: no borrow is held across the
        // branch, so the merge loop stays obviously safe.
        let extends = merged.last().is_some_and(|last| start <= last.1);
        if extends {
            let previous = merged.len() - 1;
            let grown = merged[previous].1.max(end);
            merged[previous].1 = grown;
        } else {
            merged.push((start, end));
        }
    }
    merged
}

/// ASCII-only case folding: byte lengths and offsets never move.
fn fold_ascii(text: &str) -> String {
    text.chars()
        .map(|ch| {
            if ch.is_ascii_uppercase() {
                ch.to_ascii_lowercase()
            } else {
                ch
            }
        })
        .collect()
}

/// Enter raw mode plus the alternate screen on stderr, with a safe hook.
fn init_terminal() -> io::Result<Term> {
    install_panic_hook();
    enable_raw_mode()?;
    if let Err(err) = crossterm::execute!(io::stderr(), EnterAlternateScreen) {
        let _ = disable_raw_mode();
        return Err(err);
    }
    match Terminal::new(CrosstermBackend::new(io::stderr())) {
        Ok(terminal) => Ok(terminal),
        Err(err) => {
            restore_terminal();
            Err(err)
        }
    }
}

/// Put the terminal back; never panics, so `Drop` can call it while unwinding.
fn restore_terminal() {
    let _ = disable_raw_mode();
    let _ = crossterm::execute!(io::stderr(), LeaveAlternateScreen, Show);
}

/// Restore the terminal before a panic message reaches the screen.
fn install_panic_hook() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        restore_terminal();
        previous(info);
    }));
}

/// Restores the terminal on every exit path, including `?` returns.
struct Restore;

impl Drop for Restore {
    fn drop(&mut self) {
        restore_terminal();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hit(path: &str) -> HitWire {
        HitWire {
            p: path.to_string(),
            n: path.to_string(),
            t: 0,
            s: 1,
            m: 0,
            score: 1.0,
        }
    }

    #[test]
    fn quit_keys() {
        let esc = KeyEvent::new(KeyCode::Esc, KeyModifiers::empty());
        assert!(matches!(action_for(&esc, false), Action::Quit));
        let ctrl_c = KeyEvent::new(KeyCode::Char('c'), KeyModifiers::CONTROL);
        assert!(matches!(action_for(&ctrl_c, false), Action::Quit));
        let q = KeyEvent::new(KeyCode::Char('q'), KeyModifiers::empty());
        assert!(matches!(action_for(&q, true), Action::Quit));
        assert!(matches!(action_for(&q, false), Action::Insert('q')));
    }

    #[test]
    fn navigation_keys() {
        let up = KeyEvent::new(KeyCode::Up, KeyModifiers::empty());
        assert!(matches!(action_for(&up, true), Action::Move(-1)));
        let down = KeyEvent::new(KeyCode::Down, KeyModifiers::empty());
        assert!(matches!(action_for(&down, true), Action::Move(1)));
        let ctrl_n = KeyEvent::new(KeyCode::Char('n'), KeyModifiers::CONTROL);
        assert!(matches!(action_for(&ctrl_n, true), Action::Move(1)));
        let ctrl_p = KeyEvent::new(KeyCode::Char('p'), KeyModifiers::CONTROL);
        assert!(matches!(action_for(&ctrl_p, true), Action::Move(-1)));
    }

    #[test]
    fn editing_keys() {
        let enter = KeyEvent::new(KeyCode::Enter, KeyModifiers::empty());
        assert!(matches!(action_for(&enter, false), Action::Select));
        let backspace = KeyEvent::new(KeyCode::Backspace, KeyModifiers::empty());
        assert!(matches!(action_for(&backspace, false), Action::Delete));
        let letter = KeyEvent::new(KeyCode::Char('a'), KeyModifiers::empty());
        assert!(matches!(action_for(&letter, false), Action::Insert('a')));
        let ctrl_x = KeyEvent::new(KeyCode::Char('x'), KeyModifiers::CONTROL);
        assert!(matches!(action_for(&ctrl_x, false), Action::Ignore));
    }

    #[test]
    fn key_releases_are_ignored() {
        let released = KeyEvent {
            kind: KeyEventKind::Release,
            ..KeyEvent::new(KeyCode::Down, KeyModifiers::empty())
        };
        assert!(matches!(action_for(&released, false), Action::Ignore));
    }

    #[test]
    fn needles_skip_predicates() {
        assert_eq!(query_needles("main"), ["main".to_string()]);
        assert_eq!(query_needles("ext:rs"), [".rs".to_string()]);
        assert_eq!(query_needles("path:src"), ["src".to_string()]);
        assert_eq!(
            query_needles("\"hello world\""),
            ["hello world".to_string()]
        );
        assert!(query_needles("size:>10M").is_empty());
        assert!(query_needles("mtime:<7d").is_empty());
        assert!(query_needles("!tmp").is_empty());
        assert!(query_needles("re:foo.*").is_empty());
        assert!(query_needles("main*rs").is_empty());
    }

    #[test]
    fn spans_split_on_the_matched_slice() {
        let needles = vec!["rs".to_string()];
        let spans = path_spans("src/main.rs", &needles);
        assert_eq!(spans.len(), 2);
        assert_eq!(spans[0].content, "src/main.");
        assert_eq!(spans[1].content, "rs");
        assert!(spans[0].style.fg.is_none());
        assert_eq!(spans[1].style.fg, Some(Color::Yellow));

        let plain = path_spans("Cargo.toml", &[]);
        assert_eq!(plain.len(), 1);
        assert_eq!(plain[0].content, "Cargo.toml");
    }

    #[test]
    fn movement_clamps_to_the_results() {
        let mut app = App::new();
        app.hits.push(hit("a.txt"));
        app.move_by(1);
        assert_eq!(app.state.selected(), Some(0));
        app.move_by(-1);
        assert_eq!(app.state.selected(), Some(0));
        assert_eq!(app.selected_path().as_deref(), Some("a.txt"));
    }

    #[test]
    fn clearing_the_query_clears_the_results() {
        let mut app = App::new();
        app.set_query("abc".to_string());
        app.hits.push(hit("abc.txt"));
        app.state.select(Some(0));
        assert!(app.debounce_at.is_some());

        app.set_query(String::new());
        app.busy = false;
        assert!(app.query.is_empty());
        assert!(app.hits.is_empty());
        assert!(app.state.selected().is_none());
        assert!(app.debounce_at.is_none());
        assert!(!app.should_dispatch(Instant::now()));
    }
}
