use std::collections::HashMap;
use zbus::proxy;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};

#[proxy(
    interface = "org.freedesktop.UDisks2.Manager",
    default_service = "org.freedesktop.UDisks2",
    default_path = "/org/freedesktop/UDisks2/Manager"
)]
pub trait UDisksManager {
    fn resolve_device(
        &self,
        devspec: HashMap<&str, OwnedValue>,
        options: HashMap<&str, OwnedValue>,
    ) -> zbus::Result<Vec<OwnedObjectPath>>;
}

#[zbus::proxy(
    interface = "org.freedesktop.UDisks2.Filesystem",
    default_service = "org.freedesktop.UDisks2"
)]
pub trait UDisksFilesystem {
    #[zbus(name = "Mount")]
    fn mount(
        &self,
        options: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<String>;

    #[zbus(name = "Unmount")]
    fn unmount(
        &self,
        options: std::collections::HashMap<&str, zbus::zvariant::Value<'_>>,
    ) -> zbus::Result<()>;

    #[zbus(property, name = "MountPoints")]
    fn mount_points(&self) -> zbus::Result<Vec<Vec<u8>>>;
}
