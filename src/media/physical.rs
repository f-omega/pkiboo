use procfs::process::Process;
use resolve_path::PathResolveExt;
use serde::{Deserialize, Serialize};
use std::{
    error::Error,
    path::{Path, PathBuf},
};

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct PhysicalFingerprint {
    vendor: Option<String>,
    model: Option<String>,
    serial: Option<String>,
    part_id: Option<String>,
    vendor_id: Option<String>,
    product_id: Option<String>,
    bus: Option<String>,
}

impl std::fmt::Display for PhysicalFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.serial {
            None => {
                if let Some(bus) = &self.bus {
                    write!(f, "<unidentified {bus} device>")?
                } else {
                    write!(f, "<unidentified>")?
                }
            }
            Some(serial) => {
                if let Some(bus) = &self.bus {
                    write!(f, "{bus} device {serial}")?
                } else {
                    write!(f, "{serial}")?
                }
            }
        };

        let mut delim = "(";

        if let Some(v) = &self.part_id {
            write!(f, "{delim}partition={v}")?;
            delim = ", ";
        }

        if let Some(v) = &self.vendor {
            write!(f, "{delim}vendor={v}")?;
            if let Some(id) = &self.vendor_id {
                write!(f, "<{id}>")?;
            }
            delim = ",";
        } else if let Some(v) = &self.vendor_id {
            write!(f, "{delim}vendir={v}")?;
            delim = ",";
        };

        if let Some(v) = &self.model {
            write!(f, "{delim}model={v}")?;
            if let Some(id) = &self.product_id {
                write!(f, "<{id}>")?;
            }
            delim = ",";
        } else if let Some(v) = &self.product_id {
            write!(f, "{delim}model={v}")?;
        };

        if delim == "," {
            write!(f, ")")?;
        };

        Ok(())
    }
}

impl PhysicalFingerprint {
    pub fn validate(&self) -> Result<(), Box<dyn Error>> {
        self.serial
            .as_ref()
            .ok_or::<String>("A serial number must be set on the media".into())?;
        self.part_id
            .as_ref()
            .ok_or::<String>("Must be able to identify the partition by a unique id".into())?;
        Ok(())
    }

    pub fn matches(&self, other: &PhysicalFingerprint) -> bool {
        self == other
    }

    pub fn from_udev_device(block: &udev::Device) -> Self {
        Self {
            vendor: prop(&block, "ID_VENDOR").or_else(|| attr(&block, "manufacturer")),
            model: prop(&block, "ID_MODEL").or_else(|| attr(&block, "product")),
            serial: prop(&block, "ID_SERIAL_SHORT").or_else(|| attr(&block, "serial")),
            part_id: prop(&block, "ID_FS_UUID"),
            vendor_id: attr(&block, "idVendor"),
            product_id: attr(&block, "idProduct"),
            bus: prop(&block, "ID_BUS"),
        }
    }
}

pub struct DeviceInfo {
    pub fingerprint: PhysicalFingerprint,

    pub attachment: super::backend::MediaAttachment,

    /// Whether this device shares an underlying disk with the root
    /// filesystem.
    pub system_disk: bool,
}

impl DeviceInfo {
    pub fn trusted(&self) -> bool {
        self.attachment.is_detachable() && !self.system_disk
    }
}

fn classify_attachment(removable: bool, external_bus: bool) -> super::backend::MediaAttachment {
    use super::backend::MediaAttachment;

    if removable {
        MediaAttachment::RemovableMedia
    } else if external_bus {
        MediaAttachment::ExternalBus
    } else {
        MediaAttachment::Fixed
    }
}

/// Whether udev places this device beneath a bus intended for externally
/// attached peripherals.
///
/// Checking ancestry matters because storage protocols can obscure the bus:
/// for example, a USB disk may report `ID_BUS=scsi`. We retain the property
/// check as a fallback for platforms whose udev hierarchy does not expose the
/// bus as a direct ancestor.
fn is_on_external_bus(device: &udev::Device) -> bool {
    let property_identifies_external_bus = matches!(
        prop(device, "ID_BUS").as_deref(),
        Some("usb" | "thunderbolt" | "ieee1394" | "firewire")
    );

    if property_identifies_external_bus {
        return true;
    }

    let mut current = Some(device.clone());
    while let Some(device) = current {
        if device.subsystem().is_some_and(|subsystem| {
            matches!(
                subsystem.to_string_lossy().as_ref(),
                "usb" | "thunderbolt" | "firewire"
            )
        }) {
            return true;
        }
        current = device.parent();
    }

    false
}

// utilities

pub fn prop(device: &udev::Device, name: &str) -> Option<String> {
    device
        .property_value(name)
        .map(|v| v.to_string_lossy().into_owned())
}

fn attr(device: &udev::Device, name: &str) -> Option<String> {
    let value = device
        .attribute_value(name)
        .map(|v| v.to_string_lossy().trim().to_owned());

    match value {
        None => match device.parent() {
            None => None,
            Some(parent) => attr(&parent, name),
        },
        Some(x) => Some(x),
    }
}

fn mounted_source(path: &Path) -> Result<PathBuf, Box<dyn Error>> {
    let path = path.resolve().canonicalize()?;
    let process = Process::myself()?;
    let mounts = process.mountinfo()?;
    let mount = mounts
        .iter()
        .find(|m| m.mount_point == path)
        .ok_or(format!("Could not find mount for {}", path.display()))?;
    let source = mount
        .mount_source
        .clone()
        .ok_or(format!("Could not find mount source for device {:?}", path))?;
    let source: PathBuf = PathBuf::from(&source);

    if !source.starts_with("/dev/") {
        return Err(format!(
            "Mount for {} is not backed by a device node: {}",
            path.display(),
            source.display()
        )
        .into());
    };

    Ok(source)
}

fn block_device(path: &Path) -> Result<udev::Device, Box<dyn Error>> {
    let path = path.resolve().canonicalize()?;

    let mut enumerator = udev::Enumerator::new()?;
    enumerator.match_subsystem("block")?;

    enumerator
        .scan_devices()?
        .find(|device| device.devnode() == Some(path.as_path()))
        .ok_or_else(|| format!("Could not find block device {}", path.display()).into())
}

fn containing_disk(device: &udev::Device) -> PathBuf {
    let mut current = device.clone();
    let mut result = current.syspath().to_path_buf();

    loop {
        if current
            .property_value("DEVTYPE")
            .is_some_and(|value| value == "disk")
        {
            result = current.syspath().to_path_buf();
        }

        let Some(parent) = current.parent() else {
            break;
        };
        if parent
            .subsystem()
            .is_none_or(|subsystem| subsystem != "block")
        {
            break;
        }
        current = parent;
    }

    result
}

fn root_disk() -> Option<PathBuf> {
    let mounts = Process::myself().ok()?.mountinfo().ok()?;
    let source = mounts
        .iter()
        .find(|mount| mount.mount_point == Path::new("/"))?
        .mount_source
        .as_ref()?;
    let source = PathBuf::from(source);

    if !source.starts_with("/dev/") {
        return None;
    }

    block_device(&source)
        .ok()
        .map(|device| containing_disk(&device))
}

fn device_info(block: &udev::Device) -> DeviceInfo {
    let attachment = classify_attachment(
        attr(block, "removable").as_deref() == Some("1"),
        is_on_external_bus(block),
    );
    let system_disk = root_disk().is_some_and(|root| root == containing_disk(block));

    DeviceInfo {
        fingerprint: PhysicalFingerprint::from_udev_device(block),
        attachment,
        system_disk,
    }
}

/// Retrieve physical-device information from an existing mount point.
pub fn get_device_info(path: &Path) -> Result<DeviceInfo, Box<dyn Error>> {
    get_device_info_from_device(&mounted_source(path)?)
}

/// Retrieve physical-device information directly from a block-device path.
pub fn get_device_info_from_device(path: &Path) -> Result<DeviceInfo, Box<dyn Error>> {
    let block = block_device(path)?;
    Ok(device_info(&block))
}

#[cfg(test)]
mod tests {
    use super::super::backend::MediaAttachment;
    use super::classify_attachment;

    #[test]
    fn kernel_removable_takes_precedence() {
        assert_eq!(
            classify_attachment(true, true),
            MediaAttachment::RemovableMedia
        );
    }

    #[test]
    fn usb_fixed_disk_is_external_bus_storage() {
        assert_eq!(
            classify_attachment(false, true),
            MediaAttachment::ExternalBus
        );
    }

    #[test]
    fn non_usb_fixed_disk_remains_fixed() {
        assert_eq!(classify_attachment(false, false), MediaAttachment::Fixed);
    }

    #[test]
    fn ambiguous_mmc_storage_remains_fixed() {
        // SDIO/MMC also includes soldered eMMC. Only the kernel's removable
        // bit should promote an MMC-family device to removable media.
        assert_eq!(classify_attachment(false, false), MediaAttachment::Fixed);
    }
}
