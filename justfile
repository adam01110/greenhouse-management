# List available commands.
default:
    @just --list

firmware-args := "--manifest-path src/firmware/Cargo.toml --config src/firmware/.cargo/config.toml -p greenhouse-firmware"

# Run the server executable.
server:
    cargo run -p greenhouse-server

# Build the host crates and Uno firmware.
build: host firmware

# Build the host crates without AVR target settings.
host:
    cargo build --workspace

# Build the Uno firmware for the ATmega328P.
firmware:
    cargo build {{firmware-args}}

# Check all host targets and the Uno firmware.
check: host-check firmware-check protocol-avr-check

host-check:
    cargo check --workspace --all-targets

firmware-check:
    cargo check {{firmware-args}}

# Verify the shared library also compiles without the host standard library.
protocol-avr-check:
    cargo check -p greenhouse-protocol --config src/firmware/.cargo/config.toml

# Format Rust, TOML and Nix files.
fmt:
    nix fmt

# Check both workspaces with Clippy, treating warnings as errors.
lint: host-lint firmware-lint protocol-avr-lint

host-lint:
    cargo clippy --workspace --all-targets -- -D warnings

firmware-lint:
    cargo clippy {{firmware-args}} -- -D warnings

protocol-avr-lint:
    cargo clippy -p greenhouse-protocol --config src/firmware/.cargo/config.toml -- -D warnings

# Run host tests with Nextest; AVR binaries have no host test harness.
test:
    cargo nextest run --workspace --no-tests pass

# Build optimized host binaries and Uno firmware.
release: host-release firmware-release

host-release:
    cargo build --workspace --release

firmware-release:
    cargo build {{firmware-args}} --release

# Convert the debug firmware to Intel HEX for Wokwi.
hex: firmware
    avr-objcopy -O ihex -R .eeprom src/firmware/target/avr-none/debug/greenhouse-firmware.elf src/firmware/target/avr-none/debug/greenhouse-firmware.hex

# Convert the optimized firmware to Intel HEX for Wokwi.
release-hex: firmware-release
    avr-objcopy -O ihex -R .eeprom src/firmware/target/avr-none/release/greenhouse-firmware.elf src/firmware/target/avr-none/release/greenhouse-firmware.hex

# Upload the firmware to a connected Uno.
flash:
    cargo run {{firmware-args}} --release

# Upload firmware that turns off the status LED and enters power-down mode.
stop:
    cargo run {{firmware-args}} --release --bin greenhouse-stop
