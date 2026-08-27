use super::manifest::OpenManifest;
use crate::pkiboo::Media;
use crate::ui::{Task, TaskStarterExt};
use crate::util::Name;
use std::error::Error;

/// Create and set up a new removable media
#[derive(clap::Parser)]
pub struct Args {
    #[command(flatten)]
    local: Option<CreateLocal>,

    #[command(subcommand)]
    backend: Option<CreateSpec>,

    #[arg(long)]
    name: Name<Media>,

    /// Don't require that the media is on a separate removeable block device
    #[arg(long)]
    unsafe_dont_require_removable: bool,

    /// Accept storage attached through an external bus (USB, Thunderbolt, or FireWire)
    #[arg(long)]
    allow_external_bus: bool,

    /// Don't require that the media is on a trusted device
    #[arg(long)]
    unsafe_dont_require_trusted: bool,
}

impl Args {
    fn spec(&self) -> CreateSpec {
        self.backend
            .clone()
            .unwrap_or_else(|| CreateSpec::Local(self.local.clone().unwrap()))
    }
}

#[derive(clap::Args, Clone)]
struct CreateLocal {
    /// Mount point of device
    #[arg(long, required_unless_present = "device", conflicts_with = "device")]
    path: Option<String>,

    /// Block device to identify and mount through UDisks
    #[arg(long, required_unless_present = "path", conflicts_with = "path")]
    device: Option<String>,

    /// Don't require that the media be able to be identified
    #[arg(long)]
    unsafe_dont_require_id: bool,
}

#[derive(clap::Subcommand, Clone)]
enum CreateSpec {
    /// Initialize a physical filesystem as pkiboo media
    Local(CreateLocal),
    //    S3(
}

impl CreateSpec {
    pub async fn media_id(
        &self,
    ) -> Result<(super::MediaId, super::physical::DeviceInfo), Box<dyn Error>> {
        use super::MediaId;
        use CreateSpec::*;
        match self {
            Local(local) => {
                let (device_info, path) = match (&local.path, &local.device) {
                    (Some(path), None) => {
                        let path = std::path::Path::new(path);
                        (
                            super::physical::get_device_info(path)?,
                            Some(path.to_path_buf()),
                        )
                    }
                    (None, Some(device)) => (
                        super::physical::get_device_info_from_device(std::path::Path::new(device))?,
                        None,
                    ),
                    _ => return Err("Exactly one of --path or --device is required".into()),
                };
                if !local.unsafe_dont_require_id {
                    device_info.fingerprint.validate()?;
                };
                Ok((
                    MediaId::PhysicalMedia {
                        fingerprint: device_info.fingerprint.clone(),
                        path,
                    },
                    device_info,
                ))
            }
        }
    }
}

pub(crate) async fn main<Ui: crate::Ui>(
    boo: &crate::PkiBoo<Ui>,
    _media: &super::Args,
    create: &Args,
) -> Result<(), Box<dyn Error>> {
    // Verify that the media is appropriate.
    // Requirements:
    //   1. Must be removable
    //   2. Must not be a bind mount to a non-removable file system
    //   3. Should be able to get a label and serial number from the device using standard linux commands
    let spec = create.spec();
    let (media_id, device_info) = spec.media_id().await?;
    let backend = media_id.open_backend().await?;

    // Verify that it's removable
    let (trusted, media_id) =
        boo.ui().task(format!("Identifying media {media_id}").into(),
                      async |t| {
                          use super::backend::MediaAttachment;

                          match device_info.attachment {
                              MediaAttachment::RemovableMedia => {} // Always allowed
                              MediaAttachment::ExternalBus if create.allow_external_bus => {}
                              MediaAttachment::ExternalBus => {
                                  return Err(format!(
                                      "{media_id} is attached through an external bus but is not explicitly marked removable by the kernel; pass --allow-external-bus to accept it"
                                  ).into());
                              }
                              MediaAttachment::Fixed if create.unsafe_dont_require_removable => {
                                  crate::cli_common::warn("PKI may be placed on a non-removable device. This is a bad idea because on an internet connected computer, the key data will be available to any attacker".into());
                              }
                              MediaAttachment::Fixed => {
                                  return Err(format!("{media_id} is fixed storage").into());
                              }
                          }

                          let trusted = if create.unsafe_dont_require_trusted {
                              crate::cli_common::warn("PKI may be placed on an untrusted device".into());
                              true
                          } else if !device_info.trusted() {
                              return Err(format!("{media_id} is not trusted").into());
                          } else {
                              true
                          };

                          t.set_message(format!("All checks pass for {media_id}").into()).await;
                          Ok((trusted, backend.id()))
                      }).await?;

    let mut db = boo.open_database()?;
    {
        let mut transaction = db.transaction();

        if let Some(media) = transaction.lookup_media_by_id(&media_id) {
            return Err(format!("Device already exists with label {}. If data has been wiped, consider using the 'media repair --media {0}' command", media.label).into());
        }

        // A --device source is deliberately left unmounted until all safety
        // classification and trust checks have passed.
        backend.wait_for_available().await?;
        backend.setup().await?;

        transaction.add_media(Media::new(create.name.clone(), backend.id(), trusted));

        let manifest = OpenManifest::create(backend.clone());
        manifest.save().await?;

        if let Err(error) = transaction.write_recovery_hint(backend.clone()).await {
            crate::cli_common::warn(format!(
                "Media was initialized, but its database recovery hint could not be written: {error}"
            ));
        }
    };
    backend.release().await?;
    Ok(())
}
