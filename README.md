# Greenhouse

A greenhouse irrigation project for Fontys P2, using Rust on an Arduino Uno
and a host computer.

The planned system measures soil moisture and controls watering locally on the
Uno. A Rust server receives measurements over UART, stores them in SQLite and
exposes a read-only HTTP API for a browser dashboard.

Currently, the firmware provides a timer-driven blink, a UART console and a
power-down program. The shared protocol library defines and encodes DATA/HB
messages; the server remains a scaffold. Irrigation, live telemetry, data
storage, the HTTP API and the dashboard are not implemented yet.

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

## Arduino Uno

Connect an Uno over USB and run `just flash` to upload the firmware using
ravedude. Set `RAVEDUDE_PORT` if automatic port detection cannot find the board.
`Ravedude.toml` selects the Uno board configuration.

`just stop` uploads a separate program that turns off the status LED and puts
the microcontroller into power-down mode. Upload the normal firmware again to
resume. Neither command provides irrigation control or an emergency pump stop.
