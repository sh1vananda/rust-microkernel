use crate::capability::can_send_to;
use crate::ipc::{send_message, ProcessId};
use crate::task::{agent_capabilities, AgentId};
use crate::{println, serial_println};
use alloc::{string::String, vec::Vec};
use wasmi::{Engine, Extern, Linker, Memory, Module, Store};

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
        serial_println!(
            "[WASM] Engine compiling module of length: {}",
            wasm_bytes.len()
        );
        let mut store = Store::new(&self.engine, WasmState { agent_pid });
        let module = Module::new(&self.engine, wasm_bytes)
            .map_err(|e| alloc::format!("Failed to compile module: {e}"))?;

        let mut linker = <Linker<WasmState>>::new(&self.engine);

        // Host Function: env.debug_log(ptr, len)
        // Allows the Wasm module to print to the microkernel's serial output.
        linker
            .define(
                "env",
                "debug_log",
                wasmi::Func::wrap(
                    &mut store,
                    |mut caller: wasmi::Caller<'_, WasmState>,
                     ptr: u32,
                     len: u32|
                     -> Result<(), Trap> {
                        let memory = get_memory(&mut caller)?;
                        let mut buf = alloc::vec![0u8; len as usize];
                        memory.read(&caller, ptr as usize, &mut buf).map_err(|_| {
                            Trap::from(HostError(String::from("Memory read failed")))
                        })?;

                        if let Ok(s) = core::str::from_utf8(&buf) {
                            serial_println!("[Wasm Agent {}] {}", caller.data().agent_pid, s);
                            println!("[Wasm Agent {}] {}", caller.data().agent_pid, s);
                        }
                        Ok(())
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define debug_log: {e}"))?;

        // Host Function: env.send_ipc(target_pid, msg_ptr, msg_len)
        linker
            .define(
                "env",
                "send_ipc",
                wasmi::Func::wrap(
                    &mut store,
                    |mut caller: wasmi::Caller<'_, WasmState>,
                     target_pid: u64,
                     ptr: u32,
                     len: u32|
                     -> Result<u32, Trap> {
                        let memory = get_memory(&mut caller)?;
                        let mut buf = alloc::vec![0u8; len as usize];
                        memory.read(&caller, ptr as usize, &mut buf).map_err(|_| {
                            Trap::from(HostError(String::from("Memory read failed")))
                        })?;

                        let sender_pid = ProcessId(caller.data().agent_pid);
                        let recipient_pid = ProcessId(target_pid);

                        // SECURITY CHECK: Ensure Wasm Agent is granted the Capability to message target_pid!
                        let sender_caps = agent_capabilities(AgentId(sender_pid.0));
                        if !can_send_to(&sender_caps, target_pid) {
                            serial_println!(
                                "[SECURITY] Agent {} denied send to Agent {}",
                                sender_pid.0,
                                target_pid
                            );
                            return Ok(2); // Permission Denied
                        }

                        // For now, we pass empty capabilities. In the future, the Wasm module could specify which capabilities to delegate.
                        match send_message(sender_pid, recipient_pid, buf, Vec::new()) {
                            Ok(_) => Ok(0),  // Success
                            Err(_) => Ok(1), // General Error
                        }
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define send_ipc: {e}"))?;

        // Host Function: env.tcp_request(ip_ptr: u32, port: u32, payload_ptr: u32, len: u32) -> u32
        linker
            .define(
                "env",
                "tcp_request",
                wasmi::Func::wrap(
                    &mut store,
                    |mut caller: wasmi::Caller<'_, WasmState>,
                     ip_ptr: u32,
                     port: u32,
                     ptr: u32,
                     len: u32|
                     -> Result<u32, Trap> {
                        let memory = get_memory(&mut caller)?;

                        let agent_pid = caller.data().agent_pid;
                        let caps = agent_capabilities(AgentId(agent_pid));

                        // SECURITY CHECK: Ensure Wasm Agent is granted the Network Capability!
                        if !crate::capability::can_access_network(&caps) {
                            serial_println!("[SECURITY] Agent {} denied network access", agent_pid);
                            return Ok(2); // Permission Denied
                        }

                        let mut ip_buf = [0u8; 4];
                        memory
                            .read(&caller, ip_ptr as usize, &mut ip_buf)
                            .map_err(|_| Trap::from(HostError(String::from("IP read failed"))))?;

                        let mut payload_buf = alloc::vec![0u8; len as usize];
                        memory
                            .read(&caller, ptr as usize, &mut payload_buf)
                            .map_err(|_| {
                                Trap::from(HostError(String::from("Payload read failed")))
                            })?;

                        serial_println!(
                            "[NET] Agent {} requesting TCP to {}.{}.{}.{}:{} (Payload: {} bytes)",
                            agent_pid,
                            ip_buf[0],
                            ip_buf[1],
                            ip_buf[2],
                            ip_buf[3],
                            port,
                            len
                        );

                        if let Some(ref mut net) = *crate::net::NETWORK.lock() {
                            use smoltcp::socket::tcp::{Socket, SocketBuffer};
                            use smoltcp::wire::IpAddress;

                            let rx_buffer = SocketBuffer::new(alloc::vec![0; 1500]);
                            let tx_buffer = SocketBuffer::new(alloc::vec![0; 1500]);
                            let mut socket = Socket::new(rx_buffer, tx_buffer);

                            let endpoint = (
                                IpAddress::v4(ip_buf[0], ip_buf[1], ip_buf[2], ip_buf[3]),
                                port as u16,
                            );
                            if socket.connect(net.iface.context(), endpoint, 49152).is_ok() {
                                let mut handle = net.sockets.add(socket);

                                // Force a poll to emit the bare-metal SYN frame!
                                net.iface.poll(
                                    smoltcp::time::Instant::from_millis(1),
                                    &mut net.device,
                                    &mut net.sockets,
                                );
                                serial_println!(
                                    "  -> TCP SYN packet emitted to hardware DMA ring!"
                                );

                                net.sockets.remove(handle);
                                return Ok(0); // Queued successfully
                            }
                        }

                        Ok(1) // Error
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define tcp_request: {e}"))?;

        // Host Function: env.resolve_dns(name_ptr: u32, name_len: u32, out_ip_ptr: u32) -> u32
        linker
            .define(
                "env",
                "resolve_dns",
                wasmi::Func::wrap(
                    &mut store,
                    |mut caller: wasmi::Caller<'_, WasmState>,
                     name_ptr: u32,
                     name_len: u32,
                     out_ip_ptr: u32|
                     -> Result<u32, Trap> {
                        let memory = get_memory(&mut caller)?;

                        let agent_pid = caller.data().agent_pid;
                        let caps = agent_capabilities(AgentId(agent_pid));

                        if !crate::capability::can_access_network(&caps) {
                            serial_println!("[SECURITY] Agent {} denied DNS access", agent_pid);
                            return Ok(2); // Permission Denied
                        }

                        let mut name_buf = alloc::vec![0u8; name_len as usize];
                        memory
                            .read(&caller, name_ptr as usize, &mut name_buf)
                            .map_err(|_| Trap::from(HostError(String::from("Name read failed"))))?;

                        let domain = core::str::from_utf8(&name_buf).map_err(|_| {
                            Trap::from(HostError(String::from("Invalid UTF-8 domain")))
                        })?;

                        serial_println!("[DNS] Agent {} resolving: {}", agent_pid, domain);

                        match crate::dns::resolve(domain) {
                            Some(ip) => {
                                memory
                                    .write(&mut caller, out_ip_ptr as usize, &ip)
                                    .map_err(|_| {
                                        Trap::from(HostError(String::from("IP write failed")))
                                    })?;
                                Ok(0) // Success
                            }
                            None => Ok(1), // Resolution failed
                        }
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define resolve_dns: {e}"))?;

        // Host Function: env.file_read(path_ptr, path_len, out_ptr, out_len_ptr) -> u32
        linker
            .define(
                "env",
                "file_read",
                wasmi::Func::wrap(
                    &mut store,
                    |mut caller: wasmi::Caller<'_, WasmState>,
                     path_ptr: u32,
                     path_len: u32,
                     out_ptr: u32,
                     out_len_ptr: u32|
                     -> Result<u32, Trap> {
                        let memory = get_memory(&mut caller)?;
                        let agent_pid = caller.data().agent_pid;
                        let caps = agent_capabilities(AgentId(agent_pid));

                        let mut path_buf = alloc::vec![0u8; path_len as usize];
                        memory
                            .read(&caller, path_ptr as usize, &mut path_buf)
                            .map_err(|_| Trap::from(HostError(String::from("Path read failed"))))?;
                        let path = core::str::from_utf8(&path_buf)
                            .map_err(|_| Trap::from(HostError(String::from("Invalid path"))))?;

                        if !crate::capability::can_read_file(&caps, path) {
                            serial_println!(
                                "[SECURITY] Agent {} denied file read: {}",
                                agent_pid,
                                path
                            );
                            return Ok(2);
                        }

                        match crate::vfs::open_file(path) {
                            Some(data) => {
                                let write_len = data.len() as u32;
                                memory.write(&mut caller, out_ptr as usize, &data).map_err(
                                    |_| Trap::from(HostError(String::from("Data write failed"))),
                                )?;
                                memory
                                    .write(
                                        &mut caller,
                                        out_len_ptr as usize,
                                        &write_len.to_le_bytes(),
                                    )
                                    .map_err(|_| {
                                        Trap::from(HostError(String::from("Len write failed")))
                                    })?;
                                Ok(0)
                            }
                            None => Ok(3), // Not found
                        }
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define file_read: {e}"))?;

        // Host Function: env.file_write(path_ptr, path_len, data_ptr, data_len) -> u32
        linker
            .define(
                "env",
                "file_write",
                wasmi::Func::wrap(
                    &mut store,
                    |mut caller: wasmi::Caller<'_, WasmState>,
                     path_ptr: u32,
                     path_len: u32,
                     data_ptr: u32,
                     data_len: u32|
                     -> Result<u32, Trap> {
                        let memory = get_memory(&mut caller)?;
                        let agent_pid = caller.data().agent_pid;
                        let caps = agent_capabilities(AgentId(agent_pid));

                        let mut path_buf = alloc::vec![0u8; path_len as usize];
                        memory
                            .read(&caller, path_ptr as usize, &mut path_buf)
                            .map_err(|_| Trap::from(HostError(String::from("Path read failed"))))?;
                        let path = core::str::from_utf8(&path_buf)
                            .map_err(|_| Trap::from(HostError(String::from("Invalid path"))))?;

                        if !crate::capability::can_write_file(&caps, path) {
                            serial_println!(
                                "[SECURITY] Agent {} denied file write: {}",
                                agent_pid,
                                path
                            );
                            return Ok(2);
                        }

                        let mut data_buf = alloc::vec![0u8; data_len as usize];
                        memory
                            .read(&caller, data_ptr as usize, &mut data_buf)
                            .map_err(|_| Trap::from(HostError(String::from("Data read failed"))))?;

                        if crate::vfs::write_file(path, &data_buf, agent_pid) {
                            serial_println!(
                                "[VFS] Agent {} wrote {} bytes to {}",
                                agent_pid,
                                data_len,
                                path
                            );
                            Ok(0)
                        } else {
                            Ok(1) // Write failed (e.g. read-only system file)
                        }
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define file_write: {e}"))?;

        // Host Function: env.file_list(prefix_ptr, prefix_len, out_ptr, out_len_ptr) -> u32
        linker
            .define(
                "env",
                "file_list",
                wasmi::Func::wrap(
                    &mut store,
                    |mut caller: wasmi::Caller<'_, WasmState>,
                     prefix_ptr: u32,
                     prefix_len: u32,
                     out_ptr: u32,
                     out_len_ptr: u32|
                     -> Result<u32, Trap> {
                        let memory = get_memory(&mut caller)?;
                        let agent_pid = caller.data().agent_pid;
                        let caps = agent_capabilities(AgentId(agent_pid));

                        let mut prefix_buf = alloc::vec![0u8; prefix_len as usize];
                        memory
                            .read(&caller, prefix_ptr as usize, &mut prefix_buf)
                            .map_err(|_| {
                                Trap::from(HostError(String::from("Prefix read failed")))
                            })?;
                        let prefix = core::str::from_utf8(&prefix_buf)
                            .map_err(|_| Trap::from(HostError(String::from("Invalid prefix"))))?;

                        if !crate::capability::can_read_file(&caps, prefix) {
                            serial_println!(
                                "[SECURITY] Agent {} denied file list: {}",
                                agent_pid,
                                prefix
                            );
                            return Ok(2);
                        }

                        let files = crate::vfs::list_files_prefix(prefix);
                        let listing = files.join("\n");
                        let listing_bytes = listing.as_bytes();
                        let write_len = listing_bytes.len() as u32;

                        memory
                            .write(&mut caller, out_ptr as usize, listing_bytes)
                            .map_err(|_| {
                                Trap::from(HostError(String::from("List write failed")))
                            })?;
                        memory
                            .write(&mut caller, out_len_ptr as usize, &write_len.to_le_bytes())
                            .map_err(|_| Trap::from(HostError(String::from("Len write failed"))))?;
                        Ok(0)
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define file_list: {e}"))?;

        // Host Function: env.get_time() -> u64
        linker
            .define(
                "env",
                "get_time",
                wasmi::Func::wrap(
                    &mut store,
                    |_caller: wasmi::Caller<'_, WasmState>| -> Result<u64, Trap> {
                        Ok(crate::time::unix_timestamp())
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define get_time: {e}"))?;

        // Host Function: env.get_uptime_ms() -> u64
        linker
            .define(
                "env",
                "get_uptime_ms",
                wasmi::Func::wrap(
                    &mut store,
                    |_caller: wasmi::Caller<'_, WasmState>| -> Result<u64, Trap> {
                        Ok(crate::time::uptime_ms())
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define get_uptime_ms: {e}"))?;

        // Host Function: env.request_capability(cap_type: u32, detail_ptr: u32, detail_len: u32) -> u32
        // cap_type: 0=Network, 1=FileSystem, 2=Spawn
        // detail: for FileSystem = path prefix string; for others = unused
        linker
            .define(
                "env",
                "request_capability",
                wasmi::Func::wrap(
                    &mut store,
                    |mut caller: wasmi::Caller<'_, WasmState>,
                     cap_type: u32,
                     detail_ptr: u32,
                     detail_len: u32|
                     -> Result<u32, Trap> {
                        let memory = get_memory(&mut caller)?;
                        let agent_pid = caller.data().agent_pid;

                        let mut detail_buf = alloc::vec![0u8; detail_len as usize];
                        if detail_len > 0 {
                            memory
                                .read(&caller, detail_ptr as usize, &mut detail_buf)
                                .map_err(|_| {
                                    Trap::from(HostError(String::from("Detail read failed")))
                                })?;
                        }

                        let detail_str = core::str::from_utf8(&detail_buf).unwrap_or("");

                        serial_println!(
                            "[ESCALATION] Agent {} requests capability type={} detail='{}'",
                            agent_pid,
                            cap_type,
                            detail_str
                        );

                        // Send IPC escalation to Kernel Supervisor (PID 0)
                        let ipc_msg =
                            alloc::format!("CAP_REQUEST:{}:{}:{}", agent_pid, cap_type, detail_str);
                        let sender = crate::ipc::ProcessId(agent_pid);
                        let _ = crate::ipc::send_message(
                            sender,
                            crate::ipc::KERNEL_SUPERVISOR_PID,
                            ipc_msg.into_bytes(),
                            Vec::new(),
                        );

                        // Auto-grant policy: for now, the kernel grants all requested capabilities.
                        // In production, this would check a policy engine or prompt the user.
                        match cap_type {
                            0 => {
                                // Network
                                let cap = crate::capability::create_capability(
                                    crate::capability::Capability::Network,
                                );
                                crate::task::grant_capability_to_agent(
                                    crate::task::AgentId(agent_pid),
                                    cap,
                                );
                                serial_println!(
                                    "[ESCALATION] Granted Network to Agent {}",
                                    agent_pid
                                );
                                Ok(0)
                            }
                            1 => {
                                // FileSystem
                                let prefix = if detail_str.is_empty() {
                                    "/agent/"
                                } else {
                                    detail_str
                                };
                                let cap = crate::capability::create_capability(
                                    crate::capability::Capability::FileSystem {
                                        path_prefix: String::from(prefix),
                                        read: true,
                                        write: true,
                                    },
                                );
                                crate::task::grant_capability_to_agent(
                                    crate::task::AgentId(agent_pid),
                                    cap,
                                );
                                serial_println!(
                                    "[ESCALATION] Granted FileSystem('{}') to Agent {}",
                                    prefix,
                                    agent_pid
                                );
                                Ok(0)
                            }
                            2 => {
                                // Spawn
                                let cap = crate::capability::create_capability(
                                    crate::capability::Capability::Spawn { max_children: 5 },
                                );
                                crate::task::grant_capability_to_agent(
                                    crate::task::AgentId(agent_pid),
                                    cap,
                                );
                                serial_println!(
                                    "[ESCALATION] Granted Spawn to Agent {}",
                                    agent_pid
                                );
                                Ok(0)
                            }
                            _ => {
                                serial_println!(
                                    "[ESCALATION] Unknown capability type {} from Agent {}",
                                    cap_type,
                                    agent_pid
                                );
                                Ok(1) // Unknown type
                            }
                        }
                    },
                ),
            )
            .map_err(|e| alloc::format!("Failed to define request_capability: {e}"))?;

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

        typed_func
            .call(&mut store, ())
            .map_err(|e| alloc::format!("Execution failed: {e}"))?;

        Ok(())
    }
}

// Helper to extract the single exported memory from a Caller
fn get_memory<'a>(caller: &mut wasmi::Caller<'a, WasmState>) -> Result<Memory, Trap> {
    caller
        .get_export("memory")
        .and_then(Extern::into_memory)
        .ok_or_else(|| Trap::from(HostError(String::from("Failed to find 'memory' export"))))
}
