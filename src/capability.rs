use alloc::collections::BTreeMap;
use spin::Mutex;
use crate::println;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CapabilityId(pub u64);

#[derive(Debug, Clone)]
pub enum Capability {
    Memory { base: usize, size: usize, read: bool, write: bool, execute: bool },
    Interrupt { irq: u8 },
    Port { port: u16 },
    Process { pid: u64, can_send: bool, can_receive: bool },
}

static CAPABILITY_STORE: Mutex<BTreeMap<CapabilityId, Capability>> = Mutex::new(BTreeMap::new());
static NEXT_CAP_ID: Mutex<u64> = Mutex::new(1);

pub fn init() {
    println!("Capability system initialized");
}

pub fn create_capability(cap: Capability) -> CapabilityId {
    let mut store = CAPABILITY_STORE.lock();
    let mut next_id = NEXT_CAP_ID.lock();
    
    let cap_id = CapabilityId(*next_id);
    *next_id += 1;
    
    store.insert(cap_id, cap);
    cap_id
}

pub fn validate_capability(cap_id: CapabilityId) -> Option<Capability> {
    CAPABILITY_STORE.lock().get(&cap_id).cloned()
}

pub fn revoke_capability(cap_id: CapabilityId) -> bool {
    CAPABILITY_STORE.lock().remove(&cap_id).is_some()
}
