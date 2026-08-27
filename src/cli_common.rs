use std::cell::RefCell;
use std::sync::Arc;
use anstyle::{AnsiColor, Style};
use std::io::IsTerminal;
use crate::ui::{Ui, Task, ListModel, ListView};

#[derive(Debug, Clone, Copy)]
pub(crate) struct Duration {
    days: u32
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
    let WARNING = warning_style();
    if std::io::stderr().is_terminal() {
        eprintln!("{WARNING}  ⚠️ {}{WARNING:#}", msg);
    }
}

pub(crate) fn task_list(msg: String) {
    if std::io::stderr().is_terminal() {
        eprintln!("✅ {}", msg);
    }
}

pub(crate) fn make_progress_bar(num: u64) -> indicatif::ProgressBar {
    let pb = indicatif::ProgressBar::new(num);
    pb.set_style(
        indicatif::ProgressStyle::with_template(
            "{spinner:.green} [{elapsed_precise}] [{bar:40.cyan/blue}] {pos:>3}/{len:3} {msg}"
        )
        .unwrap()
        .progress_chars("=>-"),
    );

    if !std::io::stderr().is_terminal() {
        pb.set_draw_target(indicatif::ProgressDrawTarget::hidden());
    };

    pb
}

pub(crate) fn interactive() -> bool {
    std::io::stdout().is_terminal()
}

pub struct CliBackend {
    ready: RefCell<tokio::sync::watch::Receiver<bool>>,
    progress: indicatif::MultiProgress
}

impl CliBackend {
    pub fn new(ready: tokio::sync::watch::Receiver<bool>) -> Self {
        CliBackend { progress: indicatif::MultiProgress::new(),
                     ready: ready.into() }
    }
}

#[derive(Clone)]
pub struct CliTask {
    inner: Arc<Box<indicatif::ProgressBar>>
}

pub struct CliList {
    options: crate::util::ListOptions,
    inner: Box<dyn ListModel>
}

impl Task for CliTask {
    async fn mark_complete(&self) {
        self.inner.finish()
    }

    async fn mark_error(&self, message: String) {
        self.inner.abandon_with_message(message);
    }

    async fn set_message(&self, message: String) {
        self.inner.set_message(message);
    }

    fn property_list(&self, props: Vec<(String, String)>) {
        for (k, v) in props {
            println!("{k}: {v}")
        }
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
    type TaskHandle = CliTask;
    type List = CliList;

    async fn start_task(&self, task: String) -> Self::TaskHandle {
        let pb = indicatif::ProgressBar::new_spinner();
        pb.set_message(task);
        let mpb = self.progress.add(pb);
        CliTask { inner: Arc::new(Box::new(mpb)) }
    }

    async fn ready(&self) {
        match self.ready.borrow_mut().wait_for(|r| *r).await {
            Err(e) => panic!("Could not wait"),
            Ok(_) => ()
        }
    }

    fn list<L: crate::ui::ListModel + 'static>(&self, list: L) -> Self::List {
        CliList { inner: Box::new(list), options: crate::util::ListOptions::new() }
    }
}
