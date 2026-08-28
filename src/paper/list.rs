use crate::ui::ListView;
use std::error::Error;

struct ListedPaper {
    name: String,
    key: String,
    split: String,
    share: u32,
}

impl crate::ui::ListItem for ListedPaper {
    fn column_names() -> &'static [&'static str] {
        &["name", "key", "split", "share"]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.name.clone(),
            1 => self.key.clone(),
            2 => self.split.clone(),
            3 => self.share.to_string(),
            _ => String::new(),
        }
    }
}

#[derive(clap::Args)]
pub struct Args {
    #[command(flatten)]
    list_options: crate::util::ListOptions,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let papers = db
        .papers
        .iter()
        .map(|paper| ListedPaper {
            name: paper.name.to_string(),
            key: paper.key.to_string(),
            split: paper.split.to_string(),
            share: paper.share.0,
        })
        .collect::<Vec<_>>();
    boo.ui()
        .list(papers)
        .with_options(&args.list_options)
        .display()
        .await;
    Ok(())
}
