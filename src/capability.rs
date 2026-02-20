use alloc::collections::BTreeMap;
use alloc::vec::Vec;
use spin::Mutex;
use crate::println;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct CapabilityId(pub u64);

#[derive(Debug, Clone)]
pub enum Capability {
    Memory  { base: usize, size: usize, read: bool, write: bool, execute: bool },
    Interrupt { irq: u8 },
    Port    { port: u16 },
    Process { pid: u64, can_send: bool, can_receive: bool },
    Spawn   { max_children: u32 },
    Network,
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

/// Returns true if any capability in `caps` satisfies `predicate`.
/// This is the primary enforcement function — every kernel action calls this.
pub fn find_capability<F>(caps: &[CapabilityId], predicate: F) -> bool
where
    F: Fn(&Capability) -> bool,
{
    let store = CAPABILITY_STORE.lock();
    caps.iter()
        .filter_map(|id| store.get(id))
        .any(predicate)
}

/// Convenience: check if a cap set grants readable memory access to `addr`.
pub fn can_read_memory(caps: &[CapabilityId], addr: usize) -> bool {
    find_capability(caps, |c| matches!(c,
        Capability::Memory { base, size, read: true, .. }
        if addr >= *base && addr < *base + *size
    ))
}

/// Convenience: check if a cap set grants writable memory access to `addr`.
pub fn can_write_memory(caps: &[CapabilityId], addr: usize) -> bool {
    find_capability(caps, |c| matches!(c,
        Capability::Memory { base, size, write: true, .. }
        if addr >= *base && addr < *base + *size
    ))
}

/// Convenience: check if a cap set allows sending to `target_pid`.
pub fn can_send_to(caps: &[CapabilityId], target_pid: u64) -> bool {
    find_capability(caps, |c| matches!(c,
        Capability::Process { pid, can_send: true, .. }
        if *pid == target_pid
    ))
}

pub fn can_spawn(caps: &[CapabilityId]) -> bool {
    find_capability(caps, |c| matches!(c, Capability::Spawn { .. }))
}

/// Convenience: check if a cap set allows networking layer access.
pub fn can_access_network(caps: &[CapabilityId]) -> bool {
    find_capability(caps, |c| matches!(c, Capability::Network))
}

/// Returns all resolved capabilities for debugging / display.
pub fn dump_capabilities(caps: &[CapabilityId]) -> Vec<Capability> {
    let store = CAPABILITY_STORE.lock();
    caps.iter()
        .filter_map(|id| store.get(id).cloned())
        .collect()
}
