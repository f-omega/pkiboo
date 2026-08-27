use resolve_path::PathResolveExt;
use std::{error::Error, path::{PathBuf, Path}};
use procfs::process::Process;
use serde::{Serialize, Deserialize};

#[derive(Debug, Serialize, Deserialize, PartialEq, Clone)]
pub struct PhysicalFingerprint {
    vendor: Option<String>,
    model: Option<String>,
    serial: Option<String>,
    part_id: Option<String>,
    vendor_id: Option<String>,
    product_id: Option<String>,
    bus: Option<String>
}

impl std::fmt::Display for PhysicalFingerprint {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match &self.serial {
            None => if let Some(bus) = &self.bus {
                write!(f, "<unidentified {bus} device>")?
            } else {
                write!(f, "<unidentified>")?
            },
            Some(serial) =>
                if let Some(bus) = &self.bus {
                    write!(f, "{bus} device {serial}")?
                } else {
                    write!(f, "{serial}")?
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
        self.serial.as_ref().ok_or::<String>("A serial number must be set on the media".into())?;
        self.part_id.as_ref().ok_or::<String>("Must be able to identify the partition by a unique id".into())?;
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
             bus: prop(&block, "ID_BUS")
         }
    }
}

pub struct DeviceInfo {
    pub fingerprint: PhysicalFingerprint,

    /// Whether or not we could determine this to be removable
    pub removable: bool,
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
            Some(parent) => attr(&parent, name)
        },
        Some(x) => Some(x)
    }
}

/// Retrieve a physical fingerprint from a path
pub fn get_device_info(path: &Path) -> Result<DeviceInfo, Box<dyn Error>> {
    let path = path.resolve().canonicalize()?;

    let process = Process::myself()?;
    let mounts = process.mountinfo()?;

    // TODO path could be a device node also

    // TODO allow it to not be, if explicitly allowed
    let mount = mounts.iter()
        .find(|m| m.mount_point == path)
        .ok_or(format!("Could not find mount for {}", path.display()))?;
    let source = mount.mount_source.clone().ok_or(format!("Could not find mount source for device {:?}", path))?;

    let source: PathBuf = PathBuf::from(&source);

    if !source.starts_with("/dev/") {
        return Err(format!("Mount for {} is not backed by a device node: {}", path.display(), source.display()).into());
    };

    let mut enumerator = udev::Enumerator::new()?;
    enumerator.match_subsystem("block")?; // Require this is a block device

    let block = enumerator.scan_devices()?
        .find(|d| d.devnode() == Some(source.as_path()))
        .ok_or(format!("Could not find backing block device for {}", source.display()))?;

    let id = PhysicalFingerprint::from_udev_device(&block);

    let info = DeviceInfo {
        fingerprint: id,
        removable: attr(&block, "removable") == Some("1".into())
    };
    Ok(info)
}
