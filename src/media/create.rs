use std::error::Error;
use clap::ArgGroup;
use crate::ui::{UiExt, Task};
use crate::util::Name;
use crate::pkiboo::Media;
use super::manifest::OpenManifest;

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

    /// Don't require that the media is on a trusted device
    #[arg(long)]
    unsafe_dont_require_trusted: bool
}

impl Args {
    fn spec(&self) -> CreateSpec {
        self.backend.clone().unwrap_or_else(|| CreateSpec::Local(self.local.clone().unwrap()))
    }
}

#[derive(clap::Args, Clone)]
struct CreateLocal {
    /// Mount point of device
    #[arg(long)]
    path: String,

    /// Don't require that the media be able to be identified
    #[arg(long)]
    unsafe_dont_require_id: bool,
}

#[derive(clap::Subcommand, Clone)]
enum CreateSpec {
    Local(CreateLocal),
//    S3(
}

impl CreateSpec {
    pub async fn media_id(&self) -> Result<super::MediaId, Box<dyn Error>> {
        use CreateSpec::*;
        use super::MediaId;
        match self {
            Local(local) => {
                let path = std::path::Path::new(&local.path);
                let device_info = super::physical::get_device_info(path)?;
                if !local.unsafe_dont_require_id {
                    device_info.fingerprint.validate()?;
                };
                Ok(MediaId::PhysicalMedia {
                    fingerprint: device_info.fingerprint,
                    path: Some(path.to_path_buf())
                })
            }
        }
    }
}

pub(crate) async fn main<Ui: crate::Ui>
    (boo: &crate::PkiBoo<Ui>,
     media: &super::Args,
     create: &Args) -> Result<(), Box<dyn Error>>
{
    // Verify that the media is appropriate.
    // Requirements:
    //   1. Must be removable
    //   2. Must not be a bind mount to a non-removable file system
    //   3. Should be able to get a label and serial number from the device using standard linux commands
    let spec = create.spec();
    let media_id = spec.media_id().await?;
    let backend = media_id.open_backend().await?;

    // Verify that it's removable
    let (trusted, media_id) =
        boo.ui().task(format!("Identifying media {media_id}").into(),
                      async |t| {
                          let trust_domain = backend.trust_domain().await?;
                          if create.unsafe_dont_require_removable {
                              crate::cli_common::warn("PKI may be placed on a non-removable device. This is a bad idea because on an internet connected computer, the key data will be available to any attacker".into());
                          } else if !trust_domain.removable {
                              return Err(format!("{media_id} is not removable").into());
                          };

                          let trusted = if create.unsafe_dont_require_trusted {
                              crate::cli_common::warn("PKI may be placed on an untrusted device".into());
                              true
                          } else if !trust_domain.trusted {
                              return Err(format!("{media_id} is not trusted").into());
                          } else {
                              trust_domain.trusted
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

        backend.setup().await?;

        transaction.add_media(Media::new(create.name.clone(), backend.id(), trusted));

        let manifest = OpenManifest::new(backend.clone()).await?;
        manifest.save().await?;

        transaction.backup(backend.clone()).await?;
    };
    Ok(())
}
