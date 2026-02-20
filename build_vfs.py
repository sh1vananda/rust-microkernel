import os
import tarfile
import shutil

# Path to the compiled Wasm binary
wasm_path = "hello_wasm/target/wasm32-unknown-unknown/release/hello_wasm.wasm"

# Copy the Wasm module out to the root to pack it cleanly
shutil.copyfile(wasm_path, "agent1.wasm")
shutil.copyfile(wasm_path, "agent2.wasm")

# Create a TAR archive matching USTAR format
with tarfile.open("src/archive.tar", "w") as tar:
    tar.add("agent1.wasm")
    tar.add("agent2.wasm")

# Cleanup loose files
os.remove("agent1.wasm")
os.remove("agent2.wasm")

print("Created src/archive.tar from native Rust Wasm payloads.")
