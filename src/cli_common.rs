use anstyle::{AnsiColor, Style};
use std::io::IsTerminal;

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
