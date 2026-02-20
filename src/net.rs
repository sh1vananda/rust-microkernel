use crate::rtl8139::Rtl8139;
use crate::serial_println;
use alloc::vec;
use alloc::vec::Vec;
use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::phy::{Device, DeviceCapabilities, Medium, RxToken, TxToken};
use smoltcp::time::Instant;
use smoltcp::wire::{EthernetAddress, HardwareAddress, IpAddress, IpCidr, Ipv4Address};
use spin::Mutex;

pub struct RxTokenWrapper(pub Vec<u8>);

impl RxToken for RxTokenWrapper {
    fn consume<R, F>(mut self, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        f(&mut self.0)
    }
}

pub struct TxTokenWrapper<'a> {
    device: &'a mut Rtl8139,
}

impl<'a> TxToken for TxTokenWrapper<'a> {
    fn consume<R, F>(self, len: usize, f: F) -> R
    where
        F: FnOnce(&mut [u8]) -> R,
    {
        let mut buffer = vec![0; len];
        let result = f(&mut buffer);
        self.device.tx_raw(&buffer);
        result
    }
}

impl Device for Rtl8139 {
    type RxToken<'a> = RxTokenWrapper;
    type TxToken<'a> = TxTokenWrapper<'a>;

    fn receive<'a>(
        &'a mut self,
        _timestamp: Instant,
    ) -> Option<(Self::RxToken<'a>, Self::TxToken<'a>)> {
        match self.rx_poll() {
            Some(payload) => {
                let rx = RxTokenWrapper(payload);
                let tx = TxTokenWrapper { device: self };
                Some((rx, tx))
            }
            None => None,
        }
    }

    fn transmit<'a>(&'a mut self, _timestamp: Instant) -> Option<Self::TxToken<'a>> {
        Some(TxTokenWrapper { device: self })
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.max_transmission_unit = 1500;
        caps.max_burst_size = Some(1);
        caps.medium = Medium::Ethernet;
        caps
    }
}

pub struct NetworkStack {
    pub iface: Interface,
    pub sockets: SocketSet<'static>,
    pub device: Rtl8139,
}

lazy_static::lazy_static! {
    pub static ref NETWORK: Mutex<Option<NetworkStack>> = Mutex::new(None);
}

pub fn init(mut device: Rtl8139) {
    let mac = device.mac;
    let hardware_addr = HardwareAddress::Ethernet(EthernetAddress(mac));

    let mut config = Config::new(hardware_addr);
    config.random_seed = 0x12345678; // Minimal hack for no_std PRNG randomness

    let mut iface = Interface::new(config, &mut device, Instant::from_millis(0));

    // QEMU user networking assigns 10.0.2.15 to the guest by default in typical SLIRP,
    // but just assigning a static IP directly is fastest.
    iface.update_ip_addrs(|ip_addrs| {
        ip_addrs
            .push(IpCidr::new(IpAddress::v4(10, 0, 2, 15), 24))
            .unwrap();
    });

    iface
        .routes_mut()
        .add_default_ipv4_route(Ipv4Address::new(10, 0, 2, 2))
        .unwrap();

    let sockets = SocketSet::new(vec![]);

    serial_println!("[NET] IP Stack Configured: 10.0.2.15/24 (Gateway 10.0.2.2)");

    *NETWORK.lock() = Some(NetworkStack {
        iface,
        sockets,
        device,
    });
}
