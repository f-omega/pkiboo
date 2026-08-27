use std::error::Error;
use std::path::PathBuf;

use crate::ui::{
    ListItem, ListView, PaneStarterExt, Presenter, Property, PropertyList, PropertyListView,
};

struct InspectedMedia {
    device: String,
    fingerprint: String,
    attachment: String,
    system_disk: String,
    trusted: String,
    registered_as: String,
}

impl ListItem for InspectedMedia {
    fn column_names() -> &'static [&'static str] {
        &[
            "device",
            "fingerprint",
            "attachment",
            "system disk",
            "trusted",
            "registered as",
        ]
    }

    fn get_field(&self, column: usize) -> String {
        match column {
            0 => self.device.clone(),
            1 => self.fingerprint.clone(),
            2 => self.attachment.clone(),
            3 => self.system_disk.clone(),
            4 => self.trusted.clone(),
            5 => self.registered_as.clone(),
            _ => String::new(),
        }
    }
}

#[derive(clap::Args)]
pub struct Args {
    /// Existing mount point to inspect
    #[arg(
        long,
        required_unless_present_any = ["device", "all"],
        conflicts_with_all = ["device", "all"]
    )]
    path: Option<PathBuf>,

    /// Block device to inspect without mounting it
    #[arg(
        long,
        required_unless_present_any = ["path", "all"],
        conflicts_with_all = ["path", "all"]
    )]
    device: Option<PathBuf>,

    /// List all currently attached physical block media, including untrusted media
    #[arg(long, conflicts_with_all = ["path", "device"])]
    all: bool,
}

fn registered_as(
    db: &crate::pkiboo::Db,
    fingerprint: &super::physical::PhysicalFingerprint,
) -> String {
    db.media
        .iter()
        .filter_map(|media| match &media.id {
            super::MediaId::PhysicalMedia {
                fingerprint: registered,
                ..
            } if registered == fingerprint => Some(media.label.to_string()),
            _ => None,
        })
        .collect::<Vec<_>>()
        .join(", ")
}

pub async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _media: &super::Args,
    args: &Args,
) -> Result<(), Box<dyn Error>> {
    let db = boo.open_database()?;

    if args.all {
        let mut discovered = super::physical::discover_physical_devices()?
            .into_iter()
            .map(|device| {
                let registered = registered_as(&db, &device.info.fingerprint);
                InspectedMedia {
                    device: device.path.display().to_string(),
                    fingerprint: device.info.fingerprint.to_string(),
                    attachment: device.info.attachment.to_string(),
                    system_disk: if device.info.system_disk { "yes" } else { "no" }.into(),
                    trusted: if device.info.trusted() { "yes" } else { "no" }.into(),
                    registered_as: if registered.is_empty() {
                        "not registered".into()
                    } else {
                        registered
                    },
                }
            })
            .collect::<Vec<_>>();
        discovered.sort_by(|left, right| left.device.cmp(&right.device));

        return boo
            .ui()
            .pane(
                "Attached physical media".into(),
                async |pane| -> Result<(), Box<dyn Error>> {
                    pane.list(discovered).display().await;
                    Ok(())
                },
            )
            .await;
    }

    // Inspection is intentionally read-only: identify and classify the
    // backing block device, but do not mount, initialize, or register it.
    let (source_kind, source, info) = match (&args.path, &args.device) {
        (Some(path), None) => ("mount point", path, super::physical::get_device_info(path)?),
        (None, Some(device)) => (
            "block device",
            device,
            super::physical::get_device_info_from_device(device)?,
        ),
        _ => return Err("Exactly one of --all, --path, or --device is required".into()),
    };

    boo.ui()
        .pane(
            "Media inspection".into(),
            async |pane| -> Result<(), Box<dyn Error>> {
                pane.property_list(PropertyList::new([
                    Property::new("Source type", source_kind),
                    Property::new("Source", source.display().to_string()),
                    Property::new("Fingerprint", info.fingerprint.to_string()),
                    Property::new("Attachment", info.attachment.to_string()),
                    Property::new("System disk", if info.system_disk { "yes" } else { "no" }),
                    Property::new("Trusted", if info.trusted() { "yes" } else { "no" }),
                    Property::new(
                        "Registered as",
                        match registered_as(&db, &info.fingerprint) {
                            name if name.is_empty() => "not registered".into(),
                            name => name,
                        },
                    ),
                ]))
                .display()
                .await;
                Ok(())
            },
        )
        .await
}
