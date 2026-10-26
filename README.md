# Greenhouse

This repository contains the Rust firmware and supporting code for the Fontys
P2 greenhouse project. The firmware runs on an Arduino Uno, reads an analog
input on A0 and sends readings over UART. In Wokwi, a potentiometer stands in
for the moisture sensor.

The firmware sends raw DATA frames every five seconds and a heartbeat every 60
seconds at 57600 baud. It uses a `no_std` protocol encoder and a hardware
timer for its millisecond clock. The D8 output is held low, so the simulated
pump stays off; watering control is not implemented.

Device ID `0` is a provisional simulator value. Assigning IDs from a server
would require a registration exchange, which the protocol does not define. The
server is a scaffold: SQLite storage, the HTTP API and browser dashboard are
not implemented.

See [implementation choices](docs/implementation-choices.md) for details on
the workspace, protocol and UART design, including what the checks do and
don't cover.

## Layout

| Path | Purpose |
| --- | --- |
| `src/firmware/` | `no_std` firmware for the Arduino Uno / ATmega328P |
| `src/server/` | Host application using Rust's standard library |
| `src/protocol/` | Shared `no_std`-compatible protocol library |
| `nix/` | Development shell and formatter configuration |

The host crates share the root Cargo workspace. Firmware has its own workspace
for AVR toolchain settings and size-optimized build profiles.

## Development

With Nix and flakes enabled, start from the repository root and enter the
development shell:

```console
nix develop
just
```

The shell provides nightly Rust, AVR tools, Just and Nextest. Run the commands
below from the repository root; the firmware recipes select their Cargo
manifest and AVR configuration.

| Command | Purpose |
| --- | --- |
| `just build` | Build host crates and firmware |
| `just check` | Type-check both workspaces |
| `just lint` | Run Clippy for host and firmware, rejecting warnings |
| `just test` | Run protocol tests and debug/release firmware simulations |
| `just fmt` | Format Rust, TOML and Nix files |
| `just release` | Build both workspaces with release profiles |
| `just hex` | Build debug firmware and convert it to Intel HEX |
| `just release-hex` | Build release firmware and convert it to Intel HEX |

The HEX files are written to `src/firmware/target/avr-none/debug/` or
`src/firmware/target/avr-none/release/` as `greenhouse-firmware.hex`. Wokwi
can run either file.

## Wokwi circuit

The [simulation diagram](simulation/diagram.json) connects a potentiometer to
the simulated Uno: VCC to 5V, GND to GND and SIG to A0. A blue LED on D8, with
a 330-ohm resistor and its cathode connected to GND, stands in for the pump.
HIGH turns on this LED; it does not establish the polarity of a physical relay
or pump driver.

To run the compiled Rust firmware in a new Wokwi Uno project, replace its
`diagram.json` with this file, then build with `just release-hex`. In Wokwi,
press F1 → **Upload Firmware and Start Simulation** and select
`src/firmware/target/avr-none/release/greenhouse-firmware.hex` (not the
default Arduino sketch). Click the potentiometer knob and use Home or End to
select an endpoint. The next DATA frame reports the raw reading. The pump
indicator stays off at every input because watering thresholds and safety
rules are undecided.

The Rust HEX has been tested in Chromium with raw readings 0, 512 and 1023;
DATA and HB frames appeared in the serial monitor. This does not validate a
physical moisture sensor, motor driver or watering behavior.

## Arduino Uno

Connect an Uno over USB and run `just flash` to upload the firmware with
ravedude. If automatic port detection doesn't find the board, set
`RAVEDUDE_PORT`. `Ravedude.toml` selects the Uno configuration.

`just stop` uploads a separate program that turns off the status LED and puts
the microcontroller into power-down mode. Upload the normal firmware to
resume. Neither command controls irrigation or provides an emergency pump
stop. Never connect a pump directly to a GPIO; use a suitable driver and
protection, and verify its polarity before connecting it.
