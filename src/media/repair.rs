use std::error::Error;

#[derive(clap::Parser)]
pub struct Args {
    /// Mount point of device
    #[command(flatten)]
    media_spec: super::MediaRef,

    /// Automatically remove items in the manifest that do not correspond to anything we know about
    #[arg(long)]
    unsafe_auto_remove: bool,

    /// Auto remove files that do not correspond to a key
    #[arg(long)]
    auto_remove_no_key: bool,

    /// Never remove files
    #[arg(long)]
    dont_remove: bool,

    /// Ignore extra files. This is usually safe, but can lead to accumulation of excess rubbish on disk
    #[arg(long)]
    ignore_extra_files: bool
}

pub async fn main<Ui: crate::Ui>(_boo: &crate::pkiboo::PkiBoo<Ui>, _media: &super::Args, _args: &Args) -> Result<(), Box<dyn Error>> {

    // Get the existing manifest
//    boo.ui().task(format!("Repairing media {}", media_id).into(),
//                  async |task| {
//                      let mut manifest = media.manifest().await?;
//
//                      let media_record = db.lookup_media(&manifest.media.id).ok_or_else(|| -> String { format!("Media {} is not registered. Try creating with 'media create {}'", &manifest.media.id, manifest.media.path.display()).into() })?;

//    // Collect all items from the database
//    let items = db.collect_media_items(media_record);
//
//    cli_common::task_list(format!("Identified {} items for media", items.len()).into());
//
//    let mut errors : Vec<String> = Vec::new();
//
//    // Find items that are present but ought not to be
//    let (preserved_files, not_needed_files) : (Vec<_>, Vec<_>) = {
//        let (mut needed, mut not_needed) : (Vec<_>, Vec<_>) = manifest.files.iter().partition(|file| items.iter().all(|item| item.path() != file.path));
//        if not_needed.len() > 0 && !args.unsafe_auto_remove {
//            // This is going to be a problem, because these should be removed,
//            // but removing them is generally not safe. This should not normally happen.
//
//            cli_common::task_list(format!("Found {} extra files", not_needed.len()).into());
//            if !args.ignore_extra_files && args.dont_remove {
//                panic!("Exiting because extra files were found");
//            }
//
//            // We now identify which files do not correspond to a signing
//            // key that we know. These are going to be recommended for
//            // deletion.
//            let (mut has_key, no_key): (Vec<&SignedFile>, Vec<_>) =
//                not_needed.iter().partition(|f| db.lookup_key(&f.key).is_some());
//            if cli_common::interactive() {
//                if has_key.len() > 0 {
//                    println!("The following files were signed to a key:");
//                    for file in &has_key {
//                        println!("  - {} (key {})", file.path.display(), file.key);
//                    }
//                }
//
//                if no_key.len() > 0 {
//                    println!("The following files do not correspond to a key:");
//                    for file in &no_key {
//                        println!("  - {}", file.path.display());
//                    }
//                }
//            }
//
//            if no_key.len() > 0 && args.auto_remove_no_key {
//                needed.append(&mut has_key);
//                not_needed = no_key;
//            } else {
//                needed.append(&mut not_needed);
//                not_needed = Vec::new();
//            }
//        }
//        (needed.iter().cloned().cloned().collect(), not_needed.iter().map(|f| manifest.media.file_path(f)).collect())
//    };
//
//    // Replace the manifest files
//    manifest.files = preserved_files;
//    {
//        let pb = cli_common::make_progress_bar(not_needed_files.len() as u64);
//
//        for file in not_needed_files {
//            if let Err(e) = std::fs::remove_file(&file) {
//                errors.push(format!("Could not remove {}: {}", file.display(), e).into());
//            };
//            pb.inc(1);
//        }
//    }
//
//    // Find files not present that ought to be
//    let not_present : Vec<_> = items.iter().filter(|item| manifest.files.iter().all(|f| f.path != item.path())).collect();
//    if not_present.len() > 0 {
//        todo!("Need to make files present");
//    }
//
//    manifest.save()?;
//    db.backup(manifest.media.db_path().as_path())?;
    //    Ok(())
    todo!()
}
