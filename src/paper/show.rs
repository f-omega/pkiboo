use crate::{
    pkiboo::Paper,
    ui::{PaneStarterExt, Presenter, Property, PropertyList, PropertyListView},
    util::Name,
};
use futures::future::try_join_all;
use std::error::Error;

#[derive(clap::Args)]
pub struct Args {
    /// Name of the paper artifact
    #[arg(long)]
    paper: Name<Paper>,
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _paper: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;
    let paper = db
        .lookup_paper(&args.paper)
        .ok_or_else(|| format!("Paper {} not found", args.paper))?;
    let copies = db.share_copy_count(&paper.split, paper.share);

    let details = boo.ui().pane(
        "Paper share".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.property_list(PropertyList::new([
                Property::new("Name", paper.name.to_string()),
                Property::new("Key", paper.key.to_string()),
                Property::new("Split", paper.split.to_string()),
                Property::new("Share", paper.share.0.to_string()),
                Property::new("Recorded copies", copies.to_string()),
            ]))
            .display()
            .await;
            Ok(())
        },
    );
    let metadata = boo.ui().pane(
        "Metadata".into(),
        async |pane| -> Result<(), Box<dyn Error>> {
            pane.property_list(paper.meta.properties()).display().await;
            Ok(())
        },
    );
    try_join_all([details, metadata]).await?;
    Ok(())
}
