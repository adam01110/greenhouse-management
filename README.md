# Greenhouse

A greenhouse irrigation project for Fontys P2, using Rust on an Arduino Uno
and a host computer.

The planned system measures soil moisture and controls watering locally on the
Uno. A Rust server receives measurements over UART, stores them in SQLite and
exposes a read-only HTTP API for a browser dashboard.

The firmware samples A0 and sends raw DATA frames every five seconds, plus a
heartbeat every 60 seconds, over UART at 57600 baud. It uses the shared protocol
encoder and a hardware-driven millisecond clock. In Wokwi, a potentiometer
stands in for the moisture sensor and a D8 LED represents the pump command.
That output stays off; automatic watering is not implemented.

Device ID `0` is provisional for the simulator. Server-assigned IDs need a
registration exchange that isn't defined yet. The server remains a scaffold;
data storage, the HTTP API and the dashboard are not implemented.

See [implementation choices](docs/implementation-choices.md) for the reasoning
behind the workspace, protocol and UART design, and the limits of the checks.

## Layout

| Path | Purpose |
| --- | --- |
| `src/firmware/` | Arduino Uno / ATmega328P firmware (`no_std`) |
| `src/server/` | Host application using Rust's standard library |
| `src/protocol/` | Shared library compatible with the host and firmware (`no_std`) |
| `nix/` | Development shell and formatter configuration |

The host crates share the root Cargo workspace. Firmware has a separate workspace
for its AVR toolchain settings and size-optimized build profiles.

## Development

With Nix and flakes enabled, enter the development shell from the repository root:

```console
nix develop
just
```

The shell includes nightly Rust, AVR tools, Just and Nextest. All commands below
run from the repository root; firmware recipes select their Cargo manifest and
AVR configuration explicitly.

| Command | Purpose |
| --- | --- |
| `just build` | Build host crates and firmware |
| `just check` | Type-check both workspaces |
| `just lint` | Run Clippy for host and firmware, rejecting warnings |
| `just test` | Run host tests with Nextest |
| `just fmt` | Format Rust, TOML and Nix files |
| `just release` | Build both workspaces with release profiles |
| `just hex` | Build debug firmware and convert it to Intel HEX |
| `just release-hex` | Build release firmware and convert it to Intel HEX |

HEX files are written under `src/firmware/target/avr-none/debug/` or
`src/firmware/target/avr-none/release/` as `greenhouse-firmware.hex`.
They can be used with Wokwi's Arduino Uno simulator.

## Wokwi circuit

[simulation/diagram.json](simulation/diagram.json) connects a potentiometer's
VCC to the simulated Uno's 5V, GND to GND and SIG to A0. It substitutes an analog
input for the moisture sensor. D8 drives a blue LED through a 330-ohm resistor,
with its cathode connected to GND. HIGH means on for this stand-in; that doesn't
establish the polarity of a future physical relay or pump driver.

In a new Wokwi Uno project, replace `diagram.json` with this file. Build with
`just release-hex`, then use F1 → **Upload Firmware and Start Simulation** and
select `src/firmware/target/avr-none/release/greenhouse-firmware.hex`.
This runs the compiled Rust firmware, not the default Arduino sketch.
Click the potentiometer knob, then use Home or End to select its endpoints.
The next DATA frame reports the new raw reading. The pump indicator remains
off for all inputs because watering thresholds and safety rules are undecided.
The actual Rust HEX was tested in Chromium with raw values 0, 512 and 1023;
DATA and HB frames were visible in the serial monitor. This does not validate
a physical moisture sensor, motor driver or watering behavior.

## Arduino Uno

Connect an Uno over USB and run `just flash` to upload the firmware using
ravedude. Set `RAVEDUDE_PORT` if automatic port detection cannot find the board.
`Ravedude.toml` selects the Uno board configuration.

`just stop` uploads a separate program that turns off the status LED and puts
the microcontroller into power-down mode. Upload the normal firmware again to
resume. Neither command provides irrigation control or an emergency pump stop.
