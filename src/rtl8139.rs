use alloc::vec::Vec;
use x86_64::instructions::port::Port;
use crate::serial_println;

const RTL8139_VENDOR_ID: u16 = 0x10EC;
const RTL8139_DEVICE_ID: u16 = 0x8139;

// RTL8139 standard registers
const REG_MAC05: u16 = 0x00;
const REG_MAR07: u16 = 0x08;
const REG_TSD0: u16 = 0x10;
const REG_TSAD0: u16 = 0x20;
const REG_RBSTART: u16 = 0x30;
const REG_CMD: u16 = 0x37;
const REG_IMR: u16 = 0x3C;
const REG_ISR: u16 = 0x3E;
const REG_RCR: u16 = 0x44;
const REG_CONFIG1: u16 = 0x52;

const RX_BUFFER_SIZE: usize = 8192 + 16 + 1500;
const TX_BUFFER_SIZE: usize = 2048;

#[derive(Debug)]
pub struct Rtl8139 {
    io_base: u16,
    pub mac: [u8; 6],
    phys_mem_offset: u64,
    rx_buffer: Vec<u8>,
    tx_buffers: [Vec<u8>; 4],
    tx_index: usize,
    rx_offset: usize,
}

impl Rtl8139 {
    pub fn new(io_base: u16, phys_mem_offset: u64) -> Self {
        let mut rx_buffer = Vec::with_capacity(RX_BUFFER_SIZE);
        unsafe { rx_buffer.set_len(RX_BUFFER_SIZE) };

        // Initialize 4 transmit buffers
        let tx_buffers = core::array::from_fn(|_| {
            let mut v = Vec::with_capacity(TX_BUFFER_SIZE);
            unsafe { v.set_len(TX_BUFFER_SIZE) };
            v
        });

        let mut dev = Rtl8139 {
            io_base,
            mac: [0; 6],
            phys_mem_offset,
            rx_buffer,
            tx_buffers,
            tx_index: 0,
            rx_offset: 0,
        };
        dev.read_mac();
        dev
    }

    fn virt_to_phys(&self, virt: *const u8) -> u32 {
        (virt as u64 - self.phys_mem_offset) as u32
    }

    fn read_mac(&mut self) {
        unsafe {
            for i in 0..6 {
                let mut port = Port::<u8>::new(self.io_base + REG_MAC05 + i);
                self.mac[i as usize] = port.read();
            }
        }
        serial_println!("[RTL8139] MAC Address: {:02X}:{:02X}:{:02X}:{:02X}:{:02X}:{:02X}", 
            self.mac[0], self.mac[1], self.mac[2], self.mac[3], self.mac[4], self.mac[5]);
    }

    pub fn init(&mut self) {
        unsafe {
            // 1. Power on the device
            Port::<u8>::new(self.io_base + REG_CONFIG1).write(0x00);
            
            // 2. Software Reset
            Port::<u8>::new(self.io_base + REG_CMD).write(0x10);
            while (Port::<u8>::new(self.io_base + REG_CMD).read() & 0x10) != 0 {}
            
            // 3. Setup RX Ring Buffer pointing to our physical translated memory address
            let rx_phys = self.virt_to_phys(self.rx_buffer.as_ptr());
            Port::<u32>::new(self.io_base + REG_RBSTART).write(rx_phys);
            
            // 4. Set Receive Configuration Register (Accept broadcast, physical match, wrap)
            Port::<u32>::new(self.io_base + REG_RCR).write(0x0f | (1 << 7));
            
            // 5. Enable Receiver and Transmitter
            Port::<u8>::new(self.io_base + REG_CMD).write(0x0C);
        }
        serial_println!("[RTL8139] Initialized. RX buffer physically mapped at {:#X}", self.virt_to_phys(self.rx_buffer.as_ptr()));
    }

    /// Transmit a raw ethernet payload
    pub fn tx_raw(&mut self, payload: &[u8]) {
        let ptr = self.tx_buffers[self.tx_index].as_ptr();
        let phys = self.virt_to_phys(ptr);

        let tx_buf = &mut self.tx_buffers[self.tx_index];
        tx_buf[..payload.len()].copy_from_slice(payload);

        unsafe {
            Port::<u32>::new(self.io_base + REG_TSAD0 + (self.tx_index as u16 * 4)).write(phys);
            Port::<u32>::new(self.io_base + REG_TSD0 + (self.tx_index as u16 * 4)).write(payload.len() as u32);
        }
        
        self.tx_index = (self.tx_index + 1) % 4;
    }

    /// Poll for an incoming raw ethernet payload
    pub fn rx_poll(&mut self) -> Option<Vec<u8>> {
        let cmd = unsafe { Port::<u8>::new(self.io_base + REG_CMD).read() };
        if (cmd & 1) != 0 {
            return None; // Queue Empty
        }

        let length = u16::from_le_bytes([self.rx_buffer[self.rx_offset + 2], self.rx_buffer[self.rx_offset + 3]]) as usize;
        
        let packet_offset = self.rx_offset + 4;
        let p_len = length.saturating_sub(4); // Exclude CRC at the tail end
        
        let mut packet = Vec::with_capacity(p_len);
        for i in 0..p_len {
            packet.push(self.rx_buffer[(packet_offset + i) % 8192]);
        }

        // Align offset
        self.rx_offset = (self.rx_offset + length + 4 + 3) & !3;
        if self.rx_offset >= 8192 {
            self.rx_offset -= 8192;
        }

        Some(packet)
    }
}
