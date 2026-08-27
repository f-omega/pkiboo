use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use anstyle::{AnsiColor, Style};
use std::io::IsTerminal;
use crate::ui::{ListModel, ListView, Task, TaskStarter, Ui};
use crate::ui::{TaskId, TaskTree};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Duration {
    days: u32
}

impl Duration {
    pub(crate) fn days(self) -> u32 {
        self.days
    }
}

fn warning_style() -> Style {
    Style::new()
        .bold()
        .fg_color(Some(AnsiColor::Yellow.into()))
}

impl std::str::FromStr for Duration {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let (num, unit) = s.split_at(
            s.find(|c: char| !c.is_ascii_digit()).ok_or("missing unit")?
        );

        let n: u32 = num.parse().map_err(|_| "invalid number")?;

        match unit {
            "y" => Ok(Self {days: n * 365}),
            "m" => Ok(Self {days: n * 30}),
            "d" => Ok(Self {days: n}),
            _ => Err("expected y, m, d".into())
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
}

impl CliTasks {
    fn new() -> Self {
        Self {
            progress: indicatif::MultiProgress::new(),
            state: Mutex::new(CliTaskState::default()),
        }
    }

    fn start_task(self: &Arc<Self>, parent: Option<TaskId>, message: String) -> CliTask {
        let progress_bar = indicatif::ProgressBar::new_spinner();
        progress_bar.set_message(message);

        // An unbounded indicatif bar does not advance on its own. Use the
        // spinner slot for a compact pulse that travels in both directions,
        // and explicitly drive it with a steady timer.
        let style = indicatif::ProgressStyle::with_template(
            "{prefix:.dim} 🔄 {spinner:.cyan} {msg}",
        )
        .expect("valid task progress template")
        .tick_strings(&[
            "▰▱▱",
            "▱▰▱",
            "▱▱▰",
            "▱▰▱",
        ]);

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
            progress_bar,
        }
    }

    fn finish_task(&self, id: TaskId, depth: usize, symbol: &str, message: String) {
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
        let _ = self
            .progress
            .println(format!("{indent}{symbol} {message}"));
    }
}

#[derive(Clone)]
pub struct CliTask {
    tasks: Arc<CliTasks>,
    id: TaskId,
    depth: usize,
    progress_bar: indicatif::ProgressBar,
}

pub struct CliList {
    options: crate::util::ListOptions,
    inner: Box<dyn ListModel>
}

impl Task for CliTask {
    async fn mark_complete(&self) {
        self.tasks.finish_task(
            self.id,
            self.depth,
            "✅",
            self.progress_bar.message().to_string(),
        );
    }

    async fn mark_error(&self, message: String) {
        self.tasks.finish_task(self.id, self.depth, "❌", message);
    }

    async fn set_message(&self, message: String) {
        self.progress_bar.set_message(message);
    }

    fn property_list(&self, props: Vec<(String, String)>) {
        for (k, v) in props {
            println!("{k}: {v}")
        }
    }
}

impl TaskStarter for CliTask {
    type TaskHandle = CliTask;

    async fn start_task(&self, message: String) -> Self::TaskHandle {
        self.tasks.start_task(Some(self.id), message)
    }
}

impl TaskStarter for CliBackend {
    type TaskHandle = CliTask;

    async fn start_task(&self, message: String) -> Self::TaskHandle {
        self.tasks.start_task(None, message)
    }
}

impl ListView for CliList {
    fn with_options(mut self, options: &crate::util::ListOptions) -> Self {
        self.options = options.clone();
        self
    }

    async fn display(&self) {
        use comfy_table::*;
        let mut table = Table::new();
        let mut columns : Vec<(usize, String)> = self.inner.column_names().into_iter().enumerate().collect();
        if let Some(col_filter) = &self.options.output {
            columns = col_filter.iter().map(|k| {
                match columns.iter().find(|(_, nm)| nm == k) {
                    Some(x) => x.clone(),
                    None => panic!("Column {} not found", k)
                }
            }).collect()
        }
        let (col_ixs, col_names) : (Vec<usize>, Vec<String>) = columns.into_iter().unzip();
        table.load_style(comfy_table::presets::UTF8_FULL)
            .set_content_arrangement(ContentArrangement::Dynamic)
            .set_width(80) // TODO
            .set_header(col_names);
        for i in 0..(self.inner.n_rows()) {
            let mut cells = Vec::new();
            for c in &col_ixs {
                cells.push(self.inner.get(i, *c));
            };
            table.add_row(cells);
        };
        println!("{}", table)
    }
}

impl Ui for CliBackend {
    type List = CliList;

    async fn ready(&self) {
        match self.ready.borrow_mut().wait_for(|r| *r).await {
            Err(_) => panic!("Could not wait"),
            Ok(_) => ()
        }
    }

    fn list<L: crate::ui::ListModel + 'static>(&self, list: L) -> Self::List {
        CliList { inner: Box::new(list), options: crate::util::ListOptions::new() }
    }
}
