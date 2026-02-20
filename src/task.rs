use crate::capability::CapabilityId;
use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;
use spin::Mutex;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub struct AgentId(pub u64);

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentState {
    Running,
    Terminated,
}

#[derive(Debug, Clone)]
pub struct Agent {
    pub id: AgentId,
    pub name: String,
    pub capabilities: Vec<CapabilityId>,
    pub state: AgentState,
}

struct Registry {
    agents: BTreeMap<AgentId, Agent>,
    next_id: u64,
}

impl Registry {
    const fn new() -> Self {
        Registry {
            agents: BTreeMap::new(),
            next_id: 1,
        }
    }
}

static REGISTRY: Mutex<Registry> = Mutex::new(Registry::new());

/// Spawn a new agent with the given name and pre-allocated capability set.
/// Returns its AgentId.
pub fn spawn_agent(name: &str, capabilities: Vec<CapabilityId>) -> AgentId {
    let mut reg = REGISTRY.lock();
    let id = AgentId(reg.next_id);
    reg.next_id += 1;
    reg.agents.insert(
        id,
        Agent {
            id,
            name: String::from(name),
            capabilities,
            state: AgentState::Running,
        },
    );
    id
}

/// Returns a cloned capability list for `agent_id`, or empty vec if not found.
pub fn agent_capabilities(agent_id: AgentId) -> Vec<CapabilityId> {
    REGISTRY
        .lock()
        .agents
        .get(&agent_id)
        .map(|a| a.capabilities.clone())
        .unwrap_or_default()
}

/// Returns the raw `u64` of an AgentId for use as a Process cap PID.
pub fn agent_pid(agent_id: AgentId) -> u64 {
    agent_id.0
}

/// Dynamically grant a capability to an already-running agent.
/// Used by the Kernel Supervisor's capability escalation protocol.
pub fn grant_capability_to_agent(agent_id: AgentId, cap: CapabilityId) {
    let mut reg = REGISTRY.lock();
    if let Some(agent) = reg.agents.get_mut(&agent_id) {
        agent.capabilities.push(cap);
    }
}

/// Mark an agent as terminated and revoke all its capabilities.
pub fn terminate_agent(agent_id: AgentId) {
    let mut reg = REGISTRY.lock();
    if let Some(agent) = reg.agents.get_mut(&agent_id) {
        agent.state = AgentState::Terminated;
    }
}

/// Returns agent name for display.
pub fn agent_name(agent_id: AgentId) -> Option<String> {
    REGISTRY
        .lock()
        .agents
        .get(&agent_id)
        .map(|a| a.name.clone())
}
