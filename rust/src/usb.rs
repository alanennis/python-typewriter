//! talking straight to a USB printer: find it by vendor and product id,
//! take it from the kernel driver if need be, and write to its OUT endpoint

use rusb::{DeviceHandle, Direction, GlobalContext, TransferType};
use std::io;
use std::time::Duration;

const INTERFACE: u8 = 0;
const TIMEOUT: Duration = Duration::from_secs(10);

pub struct UsbOut {
    handle: DeviceHandle<GlobalContext>,
    endpoint: u8,
    /// the kernel driver was detached and should be given back on close
    reattach: bool,
}

impl UsbOut {
    pub fn open(vendor_id: u16, product_id: u16) -> Result<Self, String> {
        let handle = rusb::open_device_with_vid_pid(vendor_id, product_id).ok_or_else(|| {
            format!("printer {vendor_id:04x}:{product_id:04x} not found on USB (or no permission)")
        })?;
        let err = |what: &str, e: rusb::Error| format!("{what}: {e}");

        let reattach = matches!(handle.kernel_driver_active(INTERFACE), Ok(true));
        if reattach {
            handle.detach_kernel_driver(INTERFACE).map_err(|e| err("detaching kernel driver", e))?;
        }
        if handle.active_configuration().unwrap_or(0) == 0 {
            handle.set_active_configuration(1).map_err(|e| err("setting configuration", e))?;
        }
        handle.claim_interface(INTERFACE).map_err(|e| err("claiming interface", e))?;

        let config = handle
            .device()
            .active_config_descriptor()
            .map_err(|e| err("reading configuration", e))?;
        let endpoint = config
            .interfaces()
            .find(|i| i.number() == INTERFACE)
            .and_then(|i| i.descriptors().next())
            .and_then(|d| {
                d.endpoint_descriptors()
                    .find(|e| e.direction() == Direction::Out && e.transfer_type() == TransferType::Bulk)
                    .map(|e| e.address())
            })
            .ok_or("no OUT endpoint found")?;

        Ok(UsbOut { handle, endpoint, reattach })
    }

    pub fn write(&self, mut data: &[u8]) -> io::Result<()> {
        while !data.is_empty() {
            let n = self
                .handle
                .write_bulk(self.endpoint, data, TIMEOUT)
                .map_err(|e| io::Error::other(e.to_string()))?;
            data = &data[n..];
        }
        Ok(())
    }
}

impl Drop for UsbOut {
    fn drop(&mut self) {
        let _ = self.handle.release_interface(INTERFACE);
        if self.reattach {
            let _ = self.handle.attach_kernel_driver(INTERFACE);
        }
    }
}
