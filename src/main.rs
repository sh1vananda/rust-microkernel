#![no_std]
#![no_main]
#![feature(custom_test_frameworks)]
#![feature(abi_x86_interrupt)]
#![feature(alloc_error_handler)]
#![allow(dead_code)]

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
mod wasm;

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

    run_wasm_demo();
}

// ── helpers ──────────────────────────────────────────────────────────────────

/// Print to both VGA screen and QEMU serial (stdout when -serial stdio).
macro_rules! log {
    ($($arg:tt)*) => {{
        println!($($arg)*);
        serial_println!($($arg)*);
    }};
}

// ── Wasm Microvisor demo ───────────────────────────────────────────────────

fn run_wasm_demo() -> ! {
    use alloc::vec;
    use capability::{Capability, create_capability};
    use task::spawn_agent;

    log!("");
    log!("============================================================");
    log!("  Rust Microkernel — OpenClaw Wasm Sandbox POC");
    log!("============================================================");
    log!("");

    log!("[SETUP] Initializing Wasm Runtime...");
    let runtime = wasm::WasmRuntime::new();

    // In a real environment, we would load the Wasm binary from a filesystem
    // or via a network boot. For this POC, we use a simple compiled Wasm array.
    // This dummy Wasm does roughly: 
    //   (module
    //     (import "env" "debug_log" (func $log (param i32 i32)))
    //     (memory (export "memory") 1)
    //     (data (i32.const 0) "Hello from Wasm Sandbox!")
    //     (func (export "_start")
    //       (call $log (i32.const 0) (i32.const 24))
    //     )
    //   )
    let dummy_wasm: &[u8] = &[
        0x00, 0x61, 0x73, 0x6d, 0x01, 0x00, 0x00, 0x00, 0x01, 0x08, 0x02, 0x60, 0x02, 0x7f, 0x7f, 0x00, 
        0x60, 0x00, 0x00, 0x02, 0x13, 0x01, 0x03, 0x65, 0x6e, 0x76, 0x09, 0x64, 0x65, 0x62, 0x75, 0x67, 
        0x5f, 0x6c, 0x6f, 0x67, 0x00, 0x00, 0x03, 0x02, 0x01, 0x01, 0x05, 0x06, 0x01, 0x01, 0x01, 0x01, 
        0x01, 0x01, 0x07, 0x0a, 0x01, 0x06, 0x5f, 0x73, 0x74, 0x61, 0x72, 0x74, 0x00, 0x01, 0x0a, 0x0a, 
        0x01, 0x08, 0x00, 0x41, 0x00, 0x41, 0x18, 0x10, 0x00, 0x0b, 0x0b, 0x1f, 0x01, 0x00, 0x41, 0x00, 
        0x0b, 0x18, 0x48, 0x65, 0x6c, 0x6c, 0x6f, 0x20, 0x66, 0x72, 0x6f, 0x6d, 0x20, 0x57, 0x61, 0x73, 
        0x6d, 0x20, 0x53, 0x61, 0x6e, 0x64, 0x62, 0x6f, 0x78, 0x21,
    ];

    log!("[SETUP] Spawning OpenClaw Core Agent...");
    
    // Give the core agent capability to spawn other agents (skills)
    let cap_spawn = create_capability(Capability::Spawn { max_children: 10 });
    let core_agent = spawn_agent("openclaw_core", vec![cap_spawn]);
    let pid = task::agent_pid(core_agent);

    log!("  Agent 'openclaw_core' created with PID: {}", pid);
    log!("[EXEC] Executing Wasm binary...");

    match runtime.execute_module(dummy_wasm, pid) {
        Ok(_) => { log!("[EXEC] Module executed successfully."); }
        Err(e) => { log!("[ERROR] Module execution failed: {}", e); }
    }

    log!("");
    log!("============================================================");
    log!("  Microvisor Halted.");
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
