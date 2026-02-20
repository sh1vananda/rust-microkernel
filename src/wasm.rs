use alloc::{string::String, vec::Vec};
use wasmi::{Engine, Linker, Module, Store, Memory, Extern};
use crate::{println, serial_println};
use crate::ipc::{ProcessId, send_message};
use crate::task::{AgentId, agent_capabilities};
use crate::capability::can_send_to;

#[derive(Debug)]
pub struct HostError(String);

impl core::fmt::Display for HostError {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        write!(f, "{}", self.0)
    }
}

// In wasmi 0.31, error types returned by host functions must implement `HostError`
impl wasmi::core::HostError for HostError {}

use wasmi::core::Trap;

// We need a dummy state for the Store. We can use this to keep track of the current agent ID if needed.
pub struct WasmState {
    pub agent_pid: u64,
}

pub struct WasmRuntime {
    engine: Engine,
}

impl WasmRuntime {
    pub fn new() -> Self {
        let engine = Engine::default();
        Self { engine }
    }

    pub fn execute_module(&self, wasm_bytes: &[u8], agent_pid: u64) -> Result<(), String> {
        serial_println!("[WASM] Engine compiling module of length: {}", wasm_bytes.len());
        let mut store = Store::new(&self.engine, WasmState { agent_pid });
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| alloc::format!("Failed to compile module: {e}"))?;

        let mut linker = <Linker<WasmState>>::new(&self.engine);

        // Host Function: env.debug_log(ptr, len)
        // Allows the Wasm module to print to the microkernel's serial output.
        linker.define("env", "debug_log", wasmi::Func::wrap(&mut store, |mut caller: wasmi::Caller<'_, WasmState>, ptr: u32, len: u32| -> Result<(), Trap> {
            let memory = get_memory(&mut caller)?;
            let mut buf = alloc::vec![0u8; len as usize];
            memory.read(&caller, ptr as usize, &mut buf).map_err(|_| Trap::from(HostError(String::from("Memory read failed"))))?;
            
            if let Ok(s) = core::str::from_utf8(&buf) {
                serial_println!("[Wasm Agent {}] {}", caller.data().agent_pid, s);
                println!("[Wasm Agent {}] {}", caller.data().agent_pid, s);
            }
            Ok(())
        })).map_err(|e| alloc::format!("Failed to define debug_log: {e}"))?;

        // Host Function: env.send_ipc(target_pid, msg_ptr, msg_len)
        linker.define("env", "send_ipc", wasmi::Func::wrap(&mut store, |mut caller: wasmi::Caller<'_, WasmState>, target_pid: u64, ptr: u32, len: u32| -> Result<u32, Trap> {
            let memory = get_memory(&mut caller)?;
            let mut buf = alloc::vec![0u8; len as usize];
            memory.read(&caller, ptr as usize, &mut buf).map_err(|_| Trap::from(HostError(String::from("Memory read failed"))))?;
            
            let sender_pid = ProcessId(caller.data().agent_pid);
            let recipient_pid = ProcessId(target_pid);
            
            // SECURITY CHECK: Ensure Wasm Agent is granted the Capability to message target_pid!
            let sender_caps = agent_capabilities(AgentId(sender_pid.0));
            if !can_send_to(&sender_caps, target_pid) {
                serial_println!("[SECURITY] Agent {} denied send to Agent {}", sender_pid.0, target_pid);
                return Ok(2); // Permission Denied
            }
            
            // For now, we pass empty capabilities. In the future, the Wasm module could specify which capabilities to delegate.
            match send_message(sender_pid, recipient_pid, buf, Vec::new()) {
                Ok(_) => Ok(0), // Success
                Err(_) => Ok(1), // General Error
            }
        })).map_err(|e| alloc::format!("Failed to define send_ipc: {e}"))?;

        // Host Function: env.tcp_request(ip_ptr: u32, port: u32, payload_ptr: u32, len: u32) -> u32
        linker.define("env", "tcp_request", wasmi::Func::wrap(&mut store, |mut caller: wasmi::Caller<'_, WasmState>, ip_ptr: u32, port: u32, ptr: u32, len: u32| -> Result<u32, Trap> {
            let memory = get_memory(&mut caller)?;
            
            let agent_pid = caller.data().agent_pid;
            let caps = agent_capabilities(AgentId(agent_pid));
            
            // SECURITY CHECK: Ensure Wasm Agent is granted the Network Capability!
            if !crate::capability::can_access_network(&caps) {
                serial_println!("[SECURITY] Agent {} denied network access", agent_pid);
                return Ok(2); // Permission Denied
            }

            let mut ip_buf = [0u8; 4];
            memory.read(&caller, ip_ptr as usize, &mut ip_buf).map_err(|_| Trap::from(HostError(String::from("IP read failed"))))?;
            
            let mut payload_buf = alloc::vec![0u8; len as usize];
            memory.read(&caller, ptr as usize, &mut payload_buf).map_err(|_| Trap::from(HostError(String::from("Payload read failed"))))?;

            serial_println!("[NET] Agent {} requesting TCP to {}.{}.{}.{}:{} (Payload: {} bytes)", 
                agent_pid, ip_buf[0], ip_buf[1], ip_buf[2], ip_buf[3], port, len);

            if let Some(ref mut net) = *crate::net::NETWORK.lock() {
                use smoltcp::socket::tcp::{Socket, SocketBuffer};
                use smoltcp::wire::IpAddress;
                
                let rx_buffer = SocketBuffer::new(alloc::vec![0; 1500]);
                let tx_buffer = SocketBuffer::new(alloc::vec![0; 1500]);
                let mut socket = Socket::new(rx_buffer, tx_buffer);
                
                let endpoint = (IpAddress::v4(ip_buf[0], ip_buf[1], ip_buf[2], ip_buf[3]), port as u16);
                if socket.connect(net.iface.context(), endpoint, 49152).is_ok() {
                    let mut handle = net.sockets.add(socket);
                    
                    // Force a poll to emit the bare-metal SYN frame!
                    net.iface.poll(smoltcp::time::Instant::from_millis(1), &mut net.device, &mut net.sockets);
                    serial_println!("  -> TCP SYN packet emitted to hardware DMA ring!");
                    
                    net.sockets.remove(handle);
                    return Ok(0); // Queued successfully
                }
            }
            
            Ok(1) // Error
        })).map_err(|e| alloc::format!("Failed to define tcp_request: {e}"))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| alloc::format!("Failed to instantiate module: {e}"))?
            .start(&mut store)
            .map_err(|e| alloc::format!("Failed to start module: {e}"))?;

        // Look for an "_start" or "main" function to execute
        let start_func = instance
            .get_func(&store, "_start")
            .or_else(|| instance.get_func(&store, "main"))
            .ok_or_else(|| String::from("No _start or main function found in module"))?;

        let typed_func = start_func
            .typed::<(), ()>(&store)
            .map_err(|e| alloc::format!("Start func has wrong signature: {e}"))?;

        typed_func.call(&mut store, ())
            .map_err(|e| alloc::format!("Execution failed: {e}"))?;

        Ok(())
    }
}

// Helper to extract the single exported memory from a Caller
fn get_memory<'a>(caller: &mut wasmi::Caller<'a, WasmState>) -> Result<Memory, Trap> {
    caller.get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| Trap::from(HostError(String::from("Failed to find 'memory' export"))))
}
