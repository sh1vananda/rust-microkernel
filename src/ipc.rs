use alloc::{collections::BTreeMap, vec::Vec};
use spin::Mutex;
use crate::capability::{CapabilityId, validate_capability};
use crate::println;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ProcessId(pub u64);

#[derive(Debug, Clone)]
pub struct Message {
    pub sender: ProcessId,
    pub data: Vec<u8>,
    pub capabilities: Vec<CapabilityId>,
}

#[derive(Debug)]
pub struct IpcEndpoint {
    pub messages: Vec<Message>,
    pub max_messages: usize,
}

static IPC_ENDPOINTS: Mutex<BTreeMap<ProcessId, IpcEndpoint>> = Mutex::new(BTreeMap::new());

pub fn init() {
    println!("IPC system initialized");
}

pub fn create_endpoint(process_id: ProcessId) -> Result<(), &'static str> {
    let mut endpoints = IPC_ENDPOINTS.lock();
    
    if endpoints.contains_key(&process_id) {
        return Err("Endpoint already exists");
    }
    
    endpoints.insert(process_id, IpcEndpoint {
        messages: Vec::new(),
        max_messages: 32,
    });
    
    Ok(())
}

pub fn send_message(
    sender: ProcessId,
    recipient: ProcessId,
    data: Vec<u8>,
    capabilities: Vec<CapabilityId>
) -> Result<(), &'static str> {
    // Validate capabilities
    for &cap_id in &capabilities {
        if validate_capability(cap_id).is_none() {
            return Err("Invalid capability");
        }
    }
    
    let mut endpoints = IPC_ENDPOINTS.lock();
    let endpoint = endpoints.get_mut(&recipient).ok_or("No such endpoint")?;
    
    if endpoint.messages.len() >= endpoint.max_messages {
        return Err("Message queue full");
    }
    
    endpoint.messages.push(Message {
        sender,
        data,
        capabilities,
    });
    
    Ok(())
}

pub fn receive_message(process_id: ProcessId) -> Option<Message> {
    let mut endpoints = IPC_ENDPOINTS.lock();
    if let Some(endpoint) = endpoints.get_mut(&process_id) {
        if !endpoint.messages.is_empty() {
            return Some(endpoint.messages.remove(0));
        }
    }
    None
}
