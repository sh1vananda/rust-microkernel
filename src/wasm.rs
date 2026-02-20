use alloc::{string::String, vec::Vec};
use wasmi::{Engine, Linker, Module, Store, Memory, Extern};
use crate::{println, serial_println};
use crate::ipc::{ProcessId, send_message};

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
        let mut store = Store::new(&self.engine, WasmState { agent_pid });
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| alloc::format!("Failed to compile module: {}", e))?;

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
        })).map_err(|e| alloc::format!("Failed to define debug_log: {}", e))?;

        // Host Function: env.send_ipc(target_pid, msg_ptr, msg_len)
        linker.define("env", "send_ipc", wasmi::Func::wrap(&mut store, |mut caller: wasmi::Caller<'_, WasmState>, target_pid: u64, ptr: u32, len: u32| -> Result<u32, Trap> {
            let memory = get_memory(&mut caller)?;
            let mut buf = alloc::vec![0u8; len as usize];
            memory.read(&caller, ptr as usize, &mut buf).map_err(|_| Trap::from(HostError(String::from("Memory read failed"))))?;
            
            let sender_pid = ProcessId(caller.data().agent_pid);
            let recipient_pid = ProcessId(target_pid);
            
            // For now, we pass empty capabilities. In the future, the Wasm module could specify which capabilities to delegate.
            match send_message(sender_pid, recipient_pid, buf, Vec::new()) {
                Ok(_) => Ok(0), // Success
                Err(_) => Ok(1), // General Error
            }
        })).map_err(|e| alloc::format!("Failed to define send_ipc: {}", e))?;

        let instance = linker
            .instantiate(&mut store, &module)
            .map_err(|e| alloc::format!("Failed to instantiate module: {}", e))?
            .start(&mut store)
            .map_err(|e| alloc::format!("Failed to start module: {}", e))?;

        // Look for an "_start" or "main" function to execute
        let start_func = instance
            .get_func(&store, "_start")
            .or_else(|| instance.get_func(&store, "main"))
            .ok_or_else(|| String::from("No _start or main function found in module"))?;

        let typed_func = start_func
            .typed::<(), ()>(&store)
            .map_err(|e| alloc::format!("Start func has wrong signature: {}", e))?;

        typed_func.call(&mut store, ())
            .map_err(|e| alloc::format!("Execution failed: {}", e))?;

        Ok(())
    }
}

// Helper to extract the single exported memory from a Caller
fn get_memory<'a>(caller: &mut wasmi::Caller<'a, WasmState>) -> Result<Memory, Trap> {
    caller.get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| Trap::from(HostError(String::from("Failed to find 'memory' export"))))
}
