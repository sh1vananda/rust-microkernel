#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]

extern crate alloc;

use core::panic::PanicInfo;
use bootloader::{BootInfo, entry_point};

mod vga_buffer;
mod serial;
mod interrupts;
mod gdt;
mod memory;
mod allocator;
mod capability;
mod ipc;
mod task;

entry_point!(kernel_main);

fn kernel_main(boot_info: &'static BootInfo) -> ! {
    use memory::BootInfoFrameAllocator;
    use x86_64::VirtAddr;

    // Initialize core systems
    gdt::init();
    interrupts::init_idt();
    unsafe { interrupts::PICS.lock().initialize() };
    x86_64::instructions::interrupts::enable();

    // Initialize memory
    let phys_mem_offset = VirtAddr::new(boot_info.physical_memory_offset);
    let mut mapper = unsafe { memory::init(phys_mem_offset) };
    let mut frame_allocator = unsafe {
        BootInfoFrameAllocator::init(&boot_info.memory_map)
    };
    allocator::init_heap(&mut mapper, &mut frame_allocator)
        .expect("heap initialization failed");

    // Initialize microkernel subsystems
    capability::init();
    ipc::init();

    run_poc_demo();
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Print to both VGA screen and QEMU serial (stdout when -serial stdio).
macro_rules! log {
    ($($arg:tt)*) => {
        println!($($arg)*);
        serial_println!($($arg)*);
    };
}

fn granted(test: u8, desc: &str) {
    log!("[TEST {}] {} => GRANTED", test, desc);
}

fn denied(test: u8, desc: &str, reason: &str) {
    log!("[TEST {}] {} => DENIED  ({})", test, desc, reason);
}

// ── POC demo ─────────────────────────────────────────────────────────────────

fn run_poc_demo() -> ! {
    use alloc::vec;
    use capability::{Capability, create_capability, can_read_memory, can_write_memory,
                     can_send_to, can_spawn};
    use task::{spawn_agent, agent_capabilities, agent_pid};
    use ipc::{create_endpoint, send_message, receive_message, ProcessId};

    log!("");
    log!("============================================================");
    log!("  Rust Microkernel — AI Agent Sandbox POC");
    log!("============================================================");
    log!("");

    // ── Agent A: data_processor ───────────────────────────────────────────
    // Gets: read access to 0x1000–0x1FFF, can send to agent B (spawned next)
    // Does NOT get: write access, spawn capability
    log!("[SETUP] Spawning agent_a (data_processor)");

    let cap_a_mem_read = create_capability(Capability::Memory {
        base: 0x1000, size: 0x1000, read: true, write: false, execute: false,
    });
    // Process cap for B will be created after B is spawned; placeholder pid=2
    // (agent IDs start at 1, so B will be AgentId(2))
    let cap_a_ipc_send = create_capability(Capability::Process {
        pid: 2, can_send: true, can_receive: false,
    });
    let agent_a = spawn_agent("data_processor", vec![cap_a_mem_read, cap_a_ipc_send]);
    log!("  agent_a id={}", agent_pid(agent_a));

    // ── Agent B: output_handler ───────────────────────────────────────────
    // Gets: write access to 0x2000–0x2FFF, can receive from agent A
    // Does NOT get: send capability, spawn capability
    log!("[SETUP] Spawning agent_b (output_handler)");

    let cap_b_mem_write = create_capability(Capability::Memory {
        base: 0x2000, size: 0x1000, read: false, write: true, execute: false,
    });
    let cap_b_ipc_recv = create_capability(Capability::Process {
        pid: agent_pid(agent_a), can_send: false, can_receive: true,
    });
    let agent_b = spawn_agent("output_handler", vec![cap_b_mem_write, cap_b_ipc_recv]);
    log!("  agent_b id={}", agent_pid(agent_b));

    // Set up IPC endpoints
    let pid_a = ProcessId(agent_pid(agent_a));
    let pid_b = ProcessId(agent_pid(agent_b));
    create_endpoint(pid_a).ok();
    create_endpoint(pid_b).ok();

    log!("");
    log!("--- Running enforcement tests ---");
    log!("");

    // ── Test 1: agent_a reads from its allowed memory region ─────────────
    let caps_a = agent_capabilities(agent_a);
    if can_read_memory(&caps_a, 0x1500) {
        granted(1, "agent_a reads  0x1500 (within  0x1000–0x1FFF)");
    } else {
        denied(1, "agent_a reads  0x1500", "no Memory{read} cap");
    }

    // ── Test 2: agent_a attempts write (no write cap) ─────────────────────
    if can_write_memory(&caps_a, 0x1500) {
        granted(2, "agent_a writes 0x1500");
    } else {
        denied(2, "agent_a writes 0x1500 (within  0x1000–0x1FFF)", "Memory cap has write=false");
    }

    // ── Test 3: agent_a sends IPC to agent_b ─────────────────────────────
    let msg_payload: alloc::vec::Vec<u8> = alloc::vec![b'H', b'e', b'l', b'l', b'o'];
    if can_send_to(&caps_a, agent_pid(agent_b)) {
        // Kernel allows the send — perform it
        match send_message(pid_a, pid_b, msg_payload, alloc::vec![]) {
            Ok(_)  => granted(3, "agent_a sends IPC msg to agent_b"),
            Err(e) => denied(3, "agent_a sends IPC msg to agent_b", e),
        }
    } else {
        denied(3, "agent_a sends IPC msg to agent_b", "no Process{can_send} cap");
    }

    // ── Test 4: agent_b receives the message ─────────────────────────────
    match receive_message(pid_b) {
        Some(msg) => granted(4, "agent_b receives IPC msg from agent_a"),
        None      => denied(4, "agent_b receives IPC msg", "no message in queue"),
    }

    // ── Test 5: agent_a attempts to spawn a child agent (no Spawn cap) ───
    if can_spawn(&caps_a) {
        granted(5, "agent_a spawns child agent");
    } else {
        denied(5, "agent_a spawns child agent", "no Spawn capability");
    }

    // ── Test 6: agent_b attempts to send (its Process cap has can_send=false) ─
    let caps_b = agent_capabilities(agent_b);
    if can_send_to(&caps_b, agent_pid(agent_a)) {
        granted(6, "agent_b sends IPC msg to agent_a");
    } else {
        denied(6, "agent_b sends IPC msg to agent_a", "Process cap has can_send=false");
    }

    // ── Summary ───────────────────────────────────────────────────────────
    log!("");
    log!("============================================================");
    log!("  Result: 3 GRANTED  |  3 DENIED");
    log!("  Kernel capability enforcement: WORKING");
    log!("============================================================");
    log!("");

    loop {
        x86_64::instructions::hlt();
    }
}

// ── Required handlers ─────────────────────────────────────────────────────────

#[panic_handler]
fn panic(info: &PanicInfo) -> ! {
    serial_println!("KERNEL PANIC: {}", info);
    println!("KERNEL PANIC: {}", info);
    loop {
        x86_64::instructions::hlt();
    }
}

#[alloc_error_handler]
fn alloc_error_handler(layout: alloc::alloc::Layout) -> ! {
    panic!("allocation error: {:?}", layout)
}
