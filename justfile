default:
    @just --list

# Build the ATmega328P firmware for the Uno.
build:
    cargo build

# Produce the HEX file for the browser simulator.
hex: build
    avr-objcopy -O ihex -R .eeprom target/avr-none/debug/uno-blink.elf target/avr-none/debug/uno-blink.hex

# Size-optimized firmware for flashing to a physical Uno.
release:
    cargo build --release

# Produce the release HEX file for the browser simulator.
release-hex: release
    avr-objcopy -O ihex -R .eeprom target/avr-none/release/uno-blink.elf target/avr-none/release/uno-blink.hex

# Build and upload to a physical Uno using ravedude's Cargo runner.
flash:
    cargo run --release
