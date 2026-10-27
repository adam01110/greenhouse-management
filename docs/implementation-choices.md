# Protocol and firmware implementation choices

The firmware samples A0, sends DATA/HB frames through the shared encoder and
reports its commanded D8 output state. A potentiometer and LED substitute for
the soil sensor and pump in Wokwi. The output stays off: this implementation
measures and reports, but doesn't run an irrigation loop. The server remains a
scaffold while its database and HTTP behavior are researched.

## Crates and build layout

All crates live under `src/` so the repository root contains project-level
configuration and commands rather than a directory for each component.
The names describe their jobs: `firmware`, `protocol` and `server`.

The [root workspace](../Cargo.toml) contains the host server and protocol
library. [Firmware](../src/firmware/Cargo.toml) has its own workspace, lockfile
and build profiles. Its [Cargo configuration](../src/firmware/.cargo/config.toml)
selects `avr-none`, the ATmega328P CPU, `ravedude` and a build of `core`.
Keeping that configuration separate prevents AVR settings from becoming the
default for host builds. The shared library uses `no_std`; the server uses
Rust's standard library. Sharing protocol code doesn't require restricting the
host application to the firmware's runtime environment.

Separate workspaces mean maintaining two lockfiles and matching lint policies.
The [Just recipes](../justfile) provide one root-level interface over them:
`just check` and `just lint` cover host code, firmware and an explicit AVR check
of the protocol library. Recipes pass the firmware manifest and configuration
paths directly, rather than changing directories. `Ravedude.toml` stays at the
root so the uploader can find its board configuration from the same location.

Firmware development and release profiles both use size optimization and LTO.
Release also uses one codegen unit. These settings target the Uno's limited
flash; they are not evidence of a measured size or speed improvement.

## Shared values, not hardware or database types

[The protocol library](../src/protocol/src/lib.rs) defines `Message::Data` and
`Message::Heartbeat`. Neither type depends on a serial port, SQLite or HTTP.
That keeps the wire representation usable without choosing a server design.

| Field | Representation | Meaning |
| --- | --- | --- |
| Device ID | `u8`, 0–255 | Identifies the device, not its serial port |
| Uptime | `u32` | Device milliseconds, not wall-clock time |
| Moisture | `MoistureRaw` | Validated raw ADC value, 0–1023 |
| Pump state | `bool` | Reported off/on state, encoded as 0/1 |

`MoistureRaw` has a private field. Its constructor returns `None` outside the
10-bit ADC range, so safe callers can't construct an out-of-range reading.
This validates the numeric domain, not sensor accuracy or whether the soil
needs water. Raw readings remain raw: host-side calibration is an agreed design
choice, but calibration and watering thresholds aren't implemented here.
A pump-state field likewise doesn't establish that a physical pump is running.

A `u32` millisecond counter wraps after about 49.7 days. These messages have no
boot ID or sequence number, so they cannot distinguish a reboot from rollover
on their own. The types represent uptime; they don't implement a clock,
ordering or duplicate detection.

## Encoding and frame boundaries

`Message::write_to` streams comma-separated ASCII fields to a `ufmt::uWrite`
writer. Both frame types end with explicit CRLF bytes:

```text
DATA,<device_id>,<uptime_ms>,<moisture_raw>,<pump_state>\r\n
HB,<device_id>,<uptime_ms>\r\n
```

Here `\r\n` shows the two terminator bytes, not four literal characters.
Numbers use decimal notation, and pump state is `0` or `1`, not `false` or
`true`. Rust debug output and enum memory layout are not wire formats.

The library uses `ufmt` 0.2, already used by firmware. It provides integer
formatting and the writer trait implemented by the pinned AVR UART driver.
There is no reason to maintain a custom decimal converter or construct an
intermediate `String` for these fields. The encoder needs no heap allocation
or complete-frame buffer. Stack and RAM usage have not been measured.

`core::fmt` also works without `std`, but its writer returns `fmt::Error`.
Using `ufmt::uWrite` lets this API return the transport's own error type without
an adapter that stores the original error elsewhere. This is an API-fit choice,
not a measured claim about AVR flash or RAM savings.

Explicit terminators avoid relying on printing-macro conventions. The pinned
`ufmt::uwriteln!` appends LF only; it doesn't insert a carriage return.
The largest encoded DATA frame is 28 bytes including CRLF:

```text
DATA,255,4294967295,1023,1\r\n
```

The largest HB frame is 19 bytes. The tests use a 28-byte capture buffer;
that isn't a production UART queue or a requirement to buffer whole frames.

### A failed write can leave a partial frame

`write_to` returns the writer's first error unchanged. It cannot undo bytes
already sent. A failure before the terminator may therefore leave only a
prefix of a frame on the transport; the write is not atomic.

[Encoding tests](../src/protocol/tests/encoding.rs) inject a failure at every
byte offset of both maximum-length frames. They check the concrete returned
error and the exact prefix already written, including failures during CRLF.
Receiver validation and recovery from incomplete input remain separate work;
there is no parser or recovery policy implemented in this crate.

## UART ownership in firmware

[The firmware library](../src/firmware/src/lib.rs) owns a mutable UART handle
and passes it directly to `ufmt`. Previously it installed the handle in the
[global console](../src/firmware/src/console.rs) and printed through that API.

The console protects access with `interrupt::free`, holding interrupts off
through formatting and synchronous transmission. Library-run-loop ownership avoids
that shared mutable state and avoids a UART-duration critical section around
output while timer interrupts run. UART writes still block the calling code;
this is not asynchronous transmission.

The pinned UART writer's error type is `Infallible`. Each frame write handles
that uninhabited error branch with an exhaustive match rather than a panic or
a blanket ignored result. This only describes the driver's API: it doesn't
prove that a receiver was connected or received the bytes.

The firmware no longer prints a startup banner: UART output contains only
protocol frames. D13 still toggles immediately and once per elapsed second as
a status indicator, without a blocking delay.

The existing console API remains available. Its formatted `println!` emits LF,
while the empty form emits CRLF. It also drops output before initialization
and ignores write errors. Its documentation now describes those differences;
it should not be mistaken for the protocol encoder.

## Sampling and telemetry scheduling

The ADC explicitly uses AVcc as its reference, matching the simulated 5V Uno
supply and the potentiometer connected between 5V and GND. A0 is read through
the pinned HAL's blocking, 10-bit ADC API. This measures the simulator's analog
input, not calibrated soil moisture. Sensor disconnection and stuck values
can't be diagnosed from the numeric range alone.

The first DATA frame is sent immediately after initialization, then at five
second intervals. HB starts after 60 seconds and repeats at that interval.
Both schedules compare wrapping elapsed milliseconds and skip missed periods
rather than transmitting a catch-up burst. ADC sampling and UART output occur
in the main loop, not in the timer ISR. A DATA timestamp is read after ADC
conversion. If DATA and HB fall due together, blocking DATA output can delay
the HB timestamp by a few milliseconds; this is not a hard-real-time scheduler.

Raw values pass through `MoistureRaw::new`. An out-of-range value would skip
that DATA sample rather than be clamped or assigned an invented ERR code.
The HAL's actual 10-bit result is in range; this guard doesn't detect a broken
sensor. HB scheduling continues independently of whether a sample is valid.

D8 is configured as an output and held LOW. The Wokwi LED has its anode fed
through a 330-ohm resistor from D8 and its cathode at GND, so HIGH represents
on. `pump.is_set_high()` reports this commanded state, not water flow or motor
feedback. It isn't inverted based on a presumed active-low relay. A real pump
needs an appropriate driver and its polarity must be confirmed before using
this firmware with it. No thresholds or automatic watering are defined.

Device ID 0 is a provisional simulator identity, not a server allocation.
Automatic assignment would need a registration/configuration exchange and
startup identity rules; none are defined by the existing telemetry frames.
The firmware doesn't persist an assigned ID or treat 0 as a reserved value.

## A hardware-driven millisecond counter

[The uptime module](../src/firmware/src/uptime.rs) adapts the pinned avr-hal
Uno millis example using the existing PAC and interrupt primitives. Timer0 runs
in CTC mode at the Uno's nominal 16 MHz, with prescaler 64 and OCR0A set to 249.
CTC counts zero through OCR0A, so each compare takes 250 timer counts, not 249:
`16,000,000 / 64 / 250 = 1000` interrupts per second. A compile-time assertion
rejects a different board clock rather than silently giving it the wrong rate.

Initialization stops Timer0, masks its interrupts, configures CTC and resets
the counter before starting it. A write-one-to-clear operation removes any
pending compare-A flag. Global interrupts are enabled only after peripheral
and ISR state is ready; there is no UART output during clock initialization.
Timer0 and its compare-A vector are reserved for uptime. This also prevents
using Timer0 PWM on Uno D5/D6; it doesn't allocate a pump pin or change
D8/D13.

The ISR only increments a `u32` with `wrapping_add`. A `Mutex<Cell<u32>>` and
short `interrupt::free` reads prevent torn multi-byte values on the 8-bit CPU.
There is no UART, ADC work or LED update inside the ISR. Consuming Timer0 by
value prevents safe code from reusing its handle; a scoped lint expectation
records why borrowing that peripheral would be the wrong ownership contract.

The LED loop compares `now.wrapping_sub(last_toggle)` with 1000, so rollover
doesn't freeze the blink. It sets the last-toggle time to the observed time,
skipping missed phases rather than trying to catch up in a burst. The CPU
still polls; this isn't a low-power scheduler. The separate power-down binary
doesn't include the timer module or enable its interrupts.

This counter starts at initialization, not the reset vector or wall-clock time.
Its first tick can vary with the shared prescaler's phase. Interrupts masked
for a full millisecond can lose ticks because the pending flag doesn't queue
multiple events. The preserved console macros must therefore not be used for
long output while this clock runs. Oscillator accuracy still needs physical
validation.

## Lints and unsafe boundaries

The root workspace forbids unsafe code in the host and protocol crates.
Firmware denies it by default but allows a reasoned, narrowly scoped exception.
The current PAC sleep-mode bit write needs such an unsafe operation. Neither
policy claims that dependencies contain no unsafe code. The firmware run loop
also needs a scoped unsafe block to enable interrupts after initialization,
outside any critical section.

The [power-down binary](../src/firmware/src/bin/stop.rs) has one such exception
for writing the documented sleep-mode register bits, with a safety explanation
next to the operation. Both firmware entry points also explain their scoped
peripheral-acquisition `unwrap`: each runs once, before peripherals have been
taken. These are local assumptions, not a project-wide waiver for panics.

Clippy's pedantic and nursery groups remain enabled, including const-function
suggestions. There is no project-wide `missing_const_for_fn` exception.
Boolean matches are allowed. Unnecessary pass-by-value and `mem::forget` warn
to flag avoidable ownership transfers and explicit leaks. `suboptimal_flops`
also warns: fused floating-point operations are permitted because measurements
have no lockstep-simulation requirement. `just lint` passes `-D warnings`,
making warnings fail that check without changing every lint to `deny` in the
manifests.

The power-down program lowers the status LED, disables interrupts and enters
ATmega328P power-down mode. It isn't pump-control logic or an emergency stop.
No pump output or watering safety behavior is established by that program.

## Verification and remaining scope

The following commands were rerun while documenting this implementation:

| Command | Result and boundary |
| --- | --- |
| `just check` | Exit 0; host, firmware and shared-library AVR checks |
| `just lint` | Exit 0; Clippy rejects warnings on the same targets |
| `just test` | Exit 0; eight protocol tests, then simulator/frame checks for both ELF profiles |
| `just hex` | Exit 0; debug firmware built and converted to Intel HEX |
| `just release-hex` | Exit 0; release firmware built and converted to HEX |

The Rust [firmware integration tests](../src/firmware-tests/src/lib.rs)
load the actual Uno ELF through `simavr-ffi`. `just test` builds debug and
release firmware, runs the eight protocol tests and the simulator/frame tests,
then repeats the firmware test crate against the release ELF.

The timer test checks Timer0 registers and D13 intervals within 512 CPU cycles
(32 microseconds) of one second. It locates MILLIS with the Rust `object` ELF
reader, injects a near-`u32::MAX` value only with interrupts enabled, and checks
rollover and subsequent LED timing. Production firmware has no test clock.

The telemetry test drives ADC0 with 0, 2500 and 5000 mV against a 5V reference.
It checks raw values 0, 511/512 and 1023, complete CRLF frames, device ID,
immediate first DATA, five-second cadence and the first heartbeat. Both
profiles produce 13 DATA frames and one HB. A GPIO observer rejects any D8
HIGH transition; DDRB/PORTB checks require a driven-low output, not an input.

The development shell includes libclang and the native libraries needed to
build the simulator binding. To run these tests separately from the repository
root:

```console
just firmware firmware-release
cargo test -p greenhouse-firmware-tests
GH_FIRMWARE_PROFILE=release cargo test -p greenhouse-firmware-tests
```

In Chromium, the compiled release HEX also ran in Wokwi with the committed
circuit. Moving the potentiometer from midpoint to both endpoints produced
raw values 512, 0 and 1023 in DATA frames. HB frames were observed at 60003 and
120003 ms, with further heartbeats continuing. The D8 indicator stayed off.
These checks exercise acquisition and transmission, not a server receiver.

Two tests cover every `u16` input to the ADC constructor. Six test encoding:
minimum and maximum DATA/HB fields, plus failures at every byte offset for each
maximum frame. These are host tests, not an AVR test harness. The separate AVR
checks establish compilation compatibility, not live UART integration.

Simulation checks the configured clock's nominal tick and rollover behavior,
not the physical oscillator, sensor accuracy, UART delivery or actuator
safety. Those properties have not been validated on a physical board.
Generating HEX doesn't upload it; no flashing was performed for these checks.
Firmware size and UART latency have not been benchmarked.

Receiver parsing and server-assigned identity remain unimplemented. ERR codes
and payloads, watering thresholds and local safety rules still need decisions.
The intended system controls watering locally rather than relying on the
server, but that loop and the server's storage/API are not implemented here.
