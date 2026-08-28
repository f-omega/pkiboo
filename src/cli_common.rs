use crate::ui::{
    ListModel, ListView, Pane, PaneStarter, Presenter, PropertyList, PropertyListView, Task,
    TaskStarter, Ui,
};
use crate::ui::{TaskId, TaskTree};
use anstyle::{AnsiColor, RgbColor, Style};
use async_trait::async_trait;
use std::cell::RefCell;
use std::collections::HashMap;
use std::io::IsTerminal;
use std::sync::{Arc, Mutex};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Duration {
    days: u32,
}

impl Duration {
    pub(crate) fn days(self) -> u32 {
        self.days
    }
}

fn warning_style() -> Style {
    Style::new().bold().fg_color(Some(AnsiColor::Yellow.into()))
}

#[derive(Clone, Copy)]
enum SemanticColor {
    Identity,
    Media,
    Artifact,
}

fn semantic_style(color: SemanticColor) -> Style {
    let rgb = match color {
        // These mirror the indigo, teal, and periwinkle paper-share palette.
        SemanticColor::Identity => RgbColor(41, 87, 184),
        SemanticColor::Media => RgbColor(0, 133, 153),
        SemanticColor::Artifact => RgbColor(102, 110, 194),
    };
    Style::new().bold().fg_color(Some(rgb.into()))
}

fn styled_semantic(value: impl std::fmt::Display, color: SemanticColor) -> String {
    let value = value.to_string();
    if std::io::stderr().is_terminal() {
        let style = semantic_style(color);
        format!("{style}{value}{style:#}")
    } else {
        value
    }
}

pub(crate) fn entity_name(value: impl std::fmt::Display) -> String {
    styled_semantic(value, SemanticColor::Identity)
}

pub(crate) fn media_name(value: impl std::fmt::Display) -> String {
    styled_semantic(value, SemanticColor::Media)
}

pub(crate) fn artifact_name(value: impl std::fmt::Display) -> String {
    styled_semantic(value, SemanticColor::Artifact)
}

pub(crate) fn success_mark() -> String {
    styled_status_mark("✓", AnsiColor::Green)
}

fn styled_status_mark(mark: &str, color: AnsiColor) -> String {
    if std::io::stderr().is_terminal() {
        let style = Style::new().bold().fg_color(Some(color.into()));
        format!("{style}{mark}{style:#}")
    } else {
        mark.to_owned()
    }
}

fn style_table_value(column: &str, value: String) -> String {
    let column = column.to_ascii_lowercase();
    if column.contains("media") || column == "label" || column == "paper" {
        media_name(value)
    } else if column.contains("path") || column.contains("file") || column.contains("split") {
        artifact_name(value)
    } else if column == "name"
        || column == "key"
        || column.contains("certificate")
        || column == "issuer"
    {
        entity_name(value)
    } else {
        value
    }
}

impl std::str::FromStr for Duration {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (num, unit) = s.split_at(
            s.find(|c: char| !c.is_ascii_digit())
                .ok_or("missing unit")?,
        );

        let n: u32 = num.parse().map_err(|_| "invalid number")?;

        match unit {
            "y" => Ok(Self { days: n * 365 }),
            "m" => Ok(Self { days: n * 30 }),
            "d" => Ok(Self { days: n }),
            _ => Err("expected y, m, d".into()),
        }
    }
}

pub(crate) fn warn(msg: String) {
    let warning = warning_style();
    if std::io::stderr().is_terminal() {
        eprintln!("{warning}  ⚠️ {}{warning:#}", msg);
    }
}

pub struct CliBackend {
    ready: RefCell<tokio::sync::watch::Receiver<bool>>,
    tasks: Arc<CliTasks>,
}

impl CliBackend {
    pub fn new(ready: tokio::sync::watch::Receiver<bool>) -> Self {
        CliBackend {
            tasks: Arc::new(CliTasks::new()),
            ready: ready.into(),
        }
    }
}

struct CliTasks {
    progress: indicatif::MultiProgress,
    state: Mutex<CliTaskState>,
}

#[derive(Default)]
struct CliTaskState {
    tree: TaskTree,

    // MultiProgress keeps only weak references. Retain each bar while its task
    // is live, then remove it after printing the final status above the live
    // progress area.
    bars: HashMap<TaskId, indicatif::ProgressBar>,

    next_pane_id: usize,
    next_pane_to_flush: usize,
    panes: HashMap<usize, CliPaneState>,
}

#[derive(Default)]
struct CliPaneState {
    lines: Vec<String>,
    finished: bool,
}

impl CliTasks {
    fn new() -> Self {
        Self {
            progress: indicatif::MultiProgress::new(),
            state: Mutex::new(CliTaskState::default()),
        }
    }

    fn start_pane(self: &Arc<Self>, title: String) -> CliPane {
        let heading = if std::io::stderr().is_terminal() {
            let style = Style::new().bold();
            format!("{style}{title}{style:#}")
        } else {
            title
        };

        let mut state = self.state.lock().expect("CLI task tree lock poisoned");
        let id = state.next_pane_id;
        state.next_pane_id += 1;
        state.panes.insert(
            id,
            CliPaneState {
                lines: vec![heading],
                finished: false,
            },
        );

        CliPane {
            tasks: self.clone(),
            id,
        }
    }

    fn print_output(&self, pane_id: Option<usize>, output: String) {
        match pane_id {
            Some(id) => {
                let mut state = self.state.lock().expect("CLI task tree lock poisoned");
                state
                    .panes
                    .get_mut(&id)
                    .expect("pane must still be open")
                    .lines
                    .push(output);
            }
            None => {
                let _ = self.progress.println(output);
            }
        }
    }

    fn finish_pane(&self, id: usize) {
        let completed_output = {
            let mut state = self.state.lock().expect("CLI task tree lock poisoned");
            state
                .panes
                .get_mut(&id)
                .expect("pane must still be open")
                .finished = true;

            let mut output = Vec::new();
            loop {
                let next = state.next_pane_to_flush;
                let is_finished = state.panes.get(&next).is_some_and(|pane| pane.finished);
                if !is_finished {
                    break;
                }

                let pane = state.panes.remove(&next).expect("finished pane exists");
                output.extend(pane.lines);
                state.next_pane_to_flush += 1;
            }
            output
        };

        for line in completed_output {
            let _ = self.progress.println(line);
        }
    }

    fn start_task(self: &Arc<Self>, parent: Option<TaskId>, message: String) -> CliTask {
        let progress_bar = indicatif::ProgressBar::new_spinner();
        let title = message;
        let detail = Arc::new(Mutex::new(None));
        progress_bar.set_message(format_task_message(&title, None));

        // An unbounded indicatif bar does not advance on its own. Use a compact
        // rotating quadrant and explicitly drive it with a steady timer.
        let style = indicatif::ProgressStyle::with_template("{prefix:.dim} {spinner:.cyan} {msg}")
            .expect("valid task progress template")
            .tick_strings(&["◴", "◷", "◶", "◵"]);

        progress_bar.set_style(style);
        progress_bar.enable_steady_tick(std::time::Duration::from_millis(90));

        let mut state = self.state.lock().expect("CLI task tree lock poisoned");
        let placement = state.tree.insert(parent);
        if placement.depth > 0 {
            progress_bar.set_prefix(format!(
                "{}↳",
                "  ".repeat(placement.depth.saturating_sub(1))
            ));
        }

        let progress_bar = self.progress.insert(placement.index, progress_bar);
        state.bars.insert(placement.id, progress_bar.clone());

        CliTask {
            tasks: self.clone(),
            id: placement.id,
            depth: placement.depth,
            title,
            detail,
            progress_bar,
        }
    }

    fn finish_task(
        &self,
        id: TaskId,
        depth: usize,
        title: &str,
        detail: Option<&str>,
        outcome: CliTaskOutcome,
    ) {
        let progress_bar = {
            let mut state = self.state.lock().expect("CLI task tree lock poisoned");

            // Error-reporting paths can attempt to finish a task more than
            // once. Only the first terminal state should produce output.
            let Some(progress_bar) = state.bars.remove(&id) else {
                return;
            };

            state.tree.remove(id);
            progress_bar
        };

        let _ = self.progress.remove(&progress_bar);

        let indent = if depth == 0 {
            String::new()
        } else {
            format!("{}↳ ", "  ".repeat(depth.saturating_sub(1)))
        };

        // MultiProgress::println writes above the currently drawn progress
        // bars, leaving one stable history row instead of a finished bar that
        // gets rendered again as later tasks update.
        let _ = self.progress.println(format!(
            "{indent}{} {}",
            outcome.styled_symbol(),
            format_terminal_task(title, detail, outcome)
        ));
    }
}

#[derive(Clone, Copy)]
enum CliTaskOutcome {
    Complete,
    Error,
    Cancelled,
}

impl CliTaskOutcome {
    fn styled_symbol(self) -> String {
        let (symbol, color) = match self {
            Self::Complete => ("✓", AnsiColor::Green),
            Self::Error => ("✗", AnsiColor::Red),
            Self::Cancelled => ("■", AnsiColor::Yellow),
        };
        styled_status_mark(symbol, color)
    }
}

fn format_task_message(title: &str, detail: Option<&str>) -> String {
    if !std::io::stderr().is_terminal() {
        return format_plain_task(title, detail);
    }

    let title_style = Style::new().bold();
    let detail_style = Style::new().dimmed();

    match detail {
        Some(detail) => {
            format!("{title_style}{title}{title_style:#}{detail_style} — {detail}{detail_style:#}")
        }
        None => format!("{title_style}{title}{title_style:#}"),
    }
}

fn format_terminal_task(title: &str, detail: Option<&str>, outcome: CliTaskOutcome) -> String {
    if !std::io::stderr().is_terminal() {
        return format_plain_task(title, detail);
    }

    let title_style = Style::new().bold();
    let detail_style = match outcome {
        CliTaskOutcome::Complete => Style::new().dimmed(),
        CliTaskOutcome::Error => Style::new().fg_color(Some(AnsiColor::Red.into())),
        CliTaskOutcome::Cancelled => Style::new().fg_color(Some(AnsiColor::Yellow.into())),
    };

    match detail {
        Some(detail) => {
            format!("{title_style}{title}{title_style:#}{detail_style} — {detail}{detail_style:#}")
        }
        None => format!("{title_style}{title}{title_style:#}"),
    }
}

fn format_plain_task(title: &str, detail: Option<&str>) -> String {
    match detail {
        Some(detail) => format!("{title} — {detail}"),
        None => title.to_owned(),
    }
}

#[derive(Clone)]
pub struct CliTask {
    tasks: Arc<CliTasks>,
    id: TaskId,
    depth: usize,
    title: String,
    detail: Arc<Mutex<Option<String>>>,
    progress_bar: indicatif::ProgressBar,
}

pub struct CliList {
    tasks: Arc<CliTasks>,
    pane_id: Option<usize>,
    options: crate::util::ListOptions,
    inner: Box<dyn ListModel>,
}

pub struct CliPropertyList {
    tasks: Arc<CliTasks>,
    pane_id: Option<usize>,
    properties: PropertyList,
}

#[derive(Clone)]
pub struct CliPane {
    tasks: Arc<CliTasks>,
    id: usize,
}

#[async_trait(?Send)]
impl Pane for CliPane {
    async fn finish(&self) {
        self.tasks.finish_pane(self.id);
    }
}

#[async_trait(?Send)]
impl Task for CliTask {
    async fn mark_complete(&self) {
        let detail = self
            .detail
            .lock()
            .expect("CLI task detail lock poisoned")
            .clone();
        self.tasks.finish_task(
            self.id,
            self.depth,
            &self.title,
            detail.as_deref(),
            CliTaskOutcome::Complete,
        );
    }

    async fn mark_error(&self, message: String) {
        self.tasks.finish_task(
            self.id,
            self.depth,
            &self.title,
            Some(&message),
            CliTaskOutcome::Error,
        );
    }

    async fn mark_cancelled(&self, message: String) {
        self.tasks.finish_task(
            self.id,
            self.depth,
            &self.title,
            Some(&message),
            CliTaskOutcome::Cancelled,
        );
    }

    async fn set_message(&self, message: String) {
        *self.detail.lock().expect("CLI task detail lock poisoned") = Some(message.clone());
        self.progress_bar
            .set_message(format_task_message(&self.title, Some(&message)));
    }
}

impl Presenter for CliTask {
    type List = CliList;
    type Properties = CliPropertyList;

    fn list<L: ListModel + 'static>(&self, list: L) -> Self::List {
        CliList {
            tasks: self.tasks.clone(),
            pane_id: None,
            inner: Box::new(list),
            options: crate::util::ListOptions::new(),
        }
    }

    fn property_list(&self, properties: PropertyList) -> Self::Properties {
        CliPropertyList {
            tasks: self.tasks.clone(),
            pane_id: None,
            properties,
        }
    }
}

#[async_trait(?Send)]
impl PaneStarter for CliTask {
    type PaneHandle = CliPane;

    async fn start_pane(&self, title: String) -> Self::PaneHandle {
        self.tasks.start_pane(title)
    }
}

#[async_trait(?Send)]
impl TaskStarter for CliTask {
    type TaskHandle = CliTask;

    async fn start_task(&self, message: String) -> Self::TaskHandle {
        self.tasks.start_task(Some(self.id), message)
    }
}

#[async_trait(?Send)]
impl TaskStarter for CliBackend {
    type TaskHandle = CliTask;

    async fn start_task(&self, message: String) -> Self::TaskHandle {
        self.tasks.start_task(None, message)
    }
}

impl Presenter for CliPane {
    type List = CliList;
    type Properties = CliPropertyList;

    fn list<L: ListModel + 'static>(&self, list: L) -> Self::List {
        CliList {
            tasks: self.tasks.clone(),
            pane_id: Some(self.id),
            inner: Box::new(list),
            options: crate::util::ListOptions::new(),
        }
    }

    fn property_list(&self, properties: PropertyList) -> Self::Properties {
        CliPropertyList {
            tasks: self.tasks.clone(),
            pane_id: Some(self.id),
            properties,
        }
    }
}

#[async_trait(?Send)]
impl ListView for CliList {
    fn with_options(mut self, options: &crate::util::ListOptions) -> Self {
        self.options = options.clone();
        self
    }

    async fn display(&self) {
        use comfy_table::*;
        let mut table = Table::new();
        let mut columns: Vec<(usize, String)> =
            self.inner.column_names().into_iter().enumerate().collect();
        if let Some(col_filter) = &self.options.output {
            columns = col_filter
                .iter()
                .map(|k| match columns.iter().find(|(_, nm)| nm == k) {
                    Some(x) => x.clone(),
                    None => panic!("Column {} not found", k),
                })
                .collect()
        }
        let (col_ixs, col_names): (Vec<usize>, Vec<String>) = columns.into_iter().unzip();
        table
            .load_style(comfy_table::presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_width(80) // TODO
            .set_header(col_names.clone());
        for i in 0..(self.inner.n_rows()) {
            let mut cells = Vec::new();
            for c in &col_ixs {
                cells.push(style_table_value(
                    &col_names[cells.len()],
                    self.inner.get(i, *c),
                ));
            }
            table.add_row(cells);
        }
        self.tasks.print_output(self.pane_id, table.to_string());
    }
}

#[async_trait(?Send)]
impl PropertyListView for CliPropertyList {
    async fn display(&self) {
        let label_width = self
            .properties
            .properties
            .iter()
            .map(|property| property.label.chars().count())
            .max()
            .unwrap_or(0);

        if let Some(title) = &self.properties.title {
            let title_style = Style::new().bold();
            let title = if std::io::stderr().is_terminal() {
                format!("{title_style}{title}{title_style:#}")
            } else {
                title.clone()
            };
            self.tasks.print_output(self.pane_id, title);
        }

        for property in &self.properties.properties {
            let label = format!("{:<label_width$}", property.label);
            let label_style = Style::new().bold().fg_color(Some(AnsiColor::Cyan.into()));
            let line = if std::io::stderr().is_terminal() {
                format!("  {label_style}{label}{label_style:#}  {}", property.value)
            } else {
                format!("  {label}  {}", property.value)
            };
            self.tasks.print_output(self.pane_id, line);
        }
    }
}

#[async_trait(?Send)]
impl Ui for CliBackend {
    async fn ready(&self) {
        match self.ready.borrow_mut().wait_for(|r| *r).await {
            Err(_) => panic!("Could not wait"),
            Ok(_) => (),
        }
    }
}

impl Presenter for CliBackend {
    type List = CliList;
    type Properties = CliPropertyList;

    fn list<L: crate::ui::ListModel + 'static>(&self, list: L) -> Self::List {
        CliList {
            tasks: self.tasks.clone(),
            pane_id: None,
            inner: Box::new(list),
            options: crate::util::ListOptions::new(),
        }
    }

    fn property_list(&self, properties: PropertyList) -> Self::Properties {
        CliPropertyList {
            tasks: self.tasks.clone(),
            pane_id: None,
            properties,
        }
    }
}

#[async_trait(?Send)]
impl PaneStarter for CliBackend {
    type PaneHandle = CliPane;

    async fn start_pane(&self, title: String) -> Self::PaneHandle {
        self.tasks.start_pane(title)
    }
}

#[cfg(test)]
mod pane_tests {
    use super::*;

    #[test]
    fn completed_panes_flush_in_creation_order() {
        let tasks = Arc::new(CliTasks::new());
        let first = tasks.start_pane("First".into());
        let second = tasks.start_pane("Second".into());

        tasks.print_output(Some(second.id), "second output".into());
        tasks.finish_pane(second.id);

        {
            let state = tasks.state.lock().expect("CLI task tree lock poisoned");
            assert_eq!(state.next_pane_to_flush, 0);
            assert_eq!(state.panes.len(), 2);
        }

        tasks.print_output(Some(first.id), "first output".into());
        tasks.finish_pane(first.id);

        let state = tasks.state.lock().expect("CLI task tree lock poisoned");
        assert_eq!(state.next_pane_to_flush, 2);
        assert!(state.panes.is_empty());
    }
}
