/// Standard syscall error codes returned by all Host Functions.
/// These provide sandbox-aware agents with descriptive failure reasons.

pub const OK: u32 = 0;
pub const ERR_GENERAL: u32 = 1;
pub const ERR_PERMISSION_DENIED: u32 = 2;
pub const ERR_NOT_FOUND: u32 = 3;
pub const ERR_NETWORK_UNREACHABLE: u32 = 4;
pub const ERR_TIMEOUT: u32 = 5;
pub const ERR_INVALID_ARGUMENT: u32 = 6;

// Capability-specific codes (100+)
pub const ERR_CAPABILITY_MISSING: u32 = 100;
pub const ERR_CAPABILITY_NETWORK: u32 = 101;
pub const ERR_CAPABILITY_FILESYSTEM: u32 = 102;
pub const ERR_CAPABILITY_SPAWN: u32 = 103;
pub const ERR_CAPABILITY_PROCESS: u32 = 104;

/// Convert an error code to a human-readable string for `env.get_last_error`.
pub fn error_message(code: u32) -> &'static str {
    match code {
        OK => "OK",
        ERR_GENERAL => "General error",
        ERR_PERMISSION_DENIED => "Permission denied",
        ERR_NOT_FOUND => "Resource not found",
        ERR_NETWORK_UNREACHABLE => "Network unreachable",
        ERR_TIMEOUT => "Operation timed out",
        ERR_INVALID_ARGUMENT => "Invalid argument",
        ERR_CAPABILITY_MISSING => "Missing required capability",
        ERR_CAPABILITY_NETWORK => "Missing Capability::Network",
        ERR_CAPABILITY_FILESYSTEM => "Missing Capability::FileSystem for this path",
        ERR_CAPABILITY_SPAWN => "Missing Capability::Spawn",
        ERR_CAPABILITY_PROCESS => "Missing Capability::Process for target PID",
        _ => "Unknown error",
    }
}
