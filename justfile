default:
    @just --list

# Build the ATmega328P firmware for the Uno.
build:
    cargo build

# Produce the HEX file for the browser simulator.
hex: build
    avr-objcopy -O ihex -R .eeprom target/avr-none/debug/uno-blink.elf target/avr-none/debug/uno-blink.hex

# Build the firmware and open de:volt; upload the generated HEX in its Sketch tab.
simulate: hex
    xdg-open 'https://sim.devoltapp.com/?guest=1'

# Size-optimized firmware for flashing to a physical Uno.
release:
    cargo build --release

# Build and upload to a physical Uno using ravedude's Cargo runner.
flash:
    cargo run --release
