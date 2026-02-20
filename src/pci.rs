use x86_64::instructions::port::Port;
use alloc::vec::Vec;

const CONFIG_ADDRESS: u16 = 0xCF8;
const CONFIG_DATA: u16 = 0xCFC;

#[derive(Debug, Clone)]
pub struct PciDevice {
    pub bus: u8,
    pub device: u8,
    pub function: u8,
    pub vendor_id: u16,
    pub device_id: u16,
    pub bar0: u32,
}

/// Reads a 32-bit dword from the PCI configuration space.
pub fn pci_read_config(bus: u8, slot: u8, func: u8, offset: u8) -> u32 {
    let address: u32 = 
        ((bus as u32) << 16) | 
        ((slot as u32) << 11) | 
        ((func as u32) << 8) | 
        (offset as u32 & 0xFC) | 
        (0x80000000u32);

    unsafe {
        Port::new(CONFIG_ADDRESS).write(address);
        Port::new(CONFIG_DATA).read()
    }
}

/// Scans the PCI buses for connected devices.
pub fn scan_buses() -> Vec<PciDevice> {
    let mut devices = Vec::new();
    
    // Scan all possible buses, slots, and functions
    for bus in 0..=255 {
        for slot in 0..32 {
            // Check Function 0 to see if device exists
            let vendor_id = (pci_read_config(bus, slot, 0, 0) & 0xFFFF) as u16;
            
            if vendor_id == 0xFFFF {
                continue; // Device doesn't exist
            }

            // Read the header type to see if it's a multi-function device
            let header_type = ((pci_read_config(bus, slot, 0, 0x0C) >> 16) & 0xFF) as u8;
            let functions = if (header_type & 0x80) != 0 { 8 } else { 1 };

            for func in 0..functions {
                let id_reg = pci_read_config(bus, slot, func, 0);
                let vend = (id_reg & 0xFFFF) as u16;
                let dev_id = (id_reg >> 16) as u16;

                if vend != 0xFFFF {
                    let bar0 = pci_read_config(bus, slot, func, 0x10);
                    devices.push(PciDevice {
                        bus,
                        device: slot,
                        function: func,
                        vendor_id: vend,
                        device_id: dev_id,
                        bar0,
                    });
                }
            }
        }
    }
    
    devices
}
