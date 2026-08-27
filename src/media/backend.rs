use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::collections::HashMap;
use std::error::Error;
use std::cell::RefCell;
use std::path::PathBuf;
use tokio::sync::Mutex;
use std::any::Any;
use async_trait::async_trait;
use futures::StreamExt;
use super::{MediaId, physical};

pub struct MediaTrustDomain {
    pub removable: bool,
    pub trusted: bool
}

#[async_trait]
pub trait Media : Sync + Send + Any + 'static {
    fn id(&self) -> MediaId;

    async fn ready(&self) -> bool;

    /// Ensure that the media is setup (create directories, buckets, etc)
    async fn setup(&self) -> Result<(), Box<dyn Error>>;

    async fn trust_domain(&self) -> Result<MediaTrustDomain, Box<dyn Error>>;

    /// Read a key in this media, returns None if key not found.
    async fn get(&self, key: &String) -> Result<Option<Vec<u8>>, Box<dyn Error>>;

    /// Atomically write a key in this media
    async fn put(&self, key: &String, bytes: &Vec<u8>) -> Result<(), Box<dyn Error>>;

    /// Wait for media to be available
    async fn wait_for_available(&self) -> Result<(), Box<dyn Error>>;
}

/// Media backed by a filesystem path
pub struct FileSystem {
    physical_drive: physical::PhysicalFingerprint,
    path: Mutex<RefCell<Option<PathBuf>>>
}

#[async_trait]
impl Media for FileSystem {
    fn id(&self) -> MediaId {
        MediaId::PhysicalMedia{
            fingerprint: self.physical_drive.clone(),
            path: None
        }
    }

    async fn ready(&self) -> bool {
        self.path.lock().await.borrow().is_some()
    }

    async fn setup(&self) -> Result<(), Box<dyn Error>> {
        self.create().await
    }

    async fn trust_domain(&self) -> Result<MediaTrustDomain, Box<dyn Error>> {
        let info = physical::get_device_info(&self.base_path().await?)?;
        Ok(MediaTrustDomain { removable: info.removable,
                              trusted: info.removable })
    }

    async fn get(&self, key: &String) -> Result<Option<Vec<u8>>, Box<dyn Error>> {
        let final_path = self.key_path(key).await?;
        match std::fs::read(&final_path) {
            Ok(d) => Ok(Some(d)),
            Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
            Err(e) => Err(Box::new(e))
        }
    }

    async fn put(&self, key: &String, bytes: &Vec<u8>) -> Result<(), Box<dyn Error>> {
        let final_path = self.key_path(key).await?;
        std::fs::create_dir_all(&final_path.parent().unwrap())?;
        std::fs::write(&final_path, bytes)?;
        Ok(())
    }

    async fn wait_for_available(&self) -> Result<(), Box<dyn Error>> {
        use super::physical::PhysicalFingerprint;
        use super::physical;

        // Attempt to find device in udev. If it's there, then we can mount
        let matches = {
            let mut enumerator = udev::Enumerator::new()?;
            enumerator.match_subsystem("block")?;

            enumerator.scan_devices()?
                .filter(|b| PhysicalFingerprint::from_udev_device(b).matches(&self.physical_drive))
                .map(|b| physical::prop(&b, "DEVNAME").unwrap())
                .collect::<Vec<_>>()
        };

        let device = if matches.is_empty() {
            let events = tokio_udev::MonitorBuilder::new()?.match_subsystem("block")?.listen()?;
            let mut async_events = tokio_udev::AsyncMonitorSocket::new(events)?;
            let device = loop {
                match async_events.next().await {
                    None => break None,
                    Some(event) => {
                        let event = event?;
                        if let udev::EventType::Add = event.event_type() {
                            let fp = PhysicalFingerprint::from_udev_device(&event.device());
                            if fp.matches(&self.physical_drive) { break Some(physical::prop(&event.device(), "DEVNAME").unwrap()) }
                        }
                    }
                }
            };
            match device {
                None => return Err(format!("Could not find device").into()),
                Some(device) => device.clone()
            }
        } else {
            matches[0].clone()
        };


        // TODO mounting backends
        let conn = zbus::Connection::system().await?;
        let manager = super::udisks::UDisksManagerProxy::new(&conn).await?;
        let devs = manager.resolve_device(HashMap::from([("path", zbus::zvariant::OwnedValue::from(zbus::zvariant::Str::from(device.as_str())))]), HashMap::new()).await?;
        if devs.len() != 1 {
            return Err(format!("Resulted in multiple devices: {:?}", devs).into())
        };

        let fs = super::udisks::UDisksFilesystemProxy::new(&conn, &devs[0]).await?;
        let mut existing_mounts = fs.mount_points().await?;
        let mount_point = if existing_mounts.is_empty() {
            PathBuf::from(&fs.mount(HashMap::new()).await?)
        } else {
            let mut mount = existing_mounts.pop().unwrap();
            mount.pop();
            PathBuf::from(&OsString::from_vec(mount))
        };

        *self.path.lock().await.borrow_mut() = Some(mount_point);

        Ok(())
    }
}

static PKI_DIR : &'static str = ".pkiboo";

impl FileSystem {
    pub fn new(physical_drive: physical::PhysicalFingerprint) -> Self {
        Self { physical_drive, path: Mutex::new(RefCell::new(None)) }
    }

    async fn pki_path(&self) -> Result<PathBuf, Box<dyn Error>> {
        Ok(self.base_path().await?.join(PKI_DIR))
    }

    async fn base_path(&self) -> Result<PathBuf, Box<dyn Error>> {
        let lock = self.path.lock().await;
        let path = lock.borrow().clone().ok_or::<String>("Filesystem is not ready".into())?;
        Ok(path)
    }

    async fn key_path(&self, key: &String) -> Result<PathBuf, Box<dyn Error>> {
        Ok(self.pki_path().await?.join(key))
    }

    /// Create the FOmega PKI dir
    async fn create(&self) -> Result<(), Box<dyn Error>> {
        std::fs::create_dir_all(&self.pki_path().await?)?;
        Ok(())
    }

    pub fn with_path(mut self, path: PathBuf) -> Self {
        self.path = Mutex::new(RefCell::new(Some(path)));
        self
    }
}
