use crate::Simulator;

mod frame;
mod probe;

use probe::{TelemetryProbe, ADC_INPUTS};

#[test]
fn telemetry_adc_uart_timing_and_pump_output() {
    let mut simulator = Simulator::load();
    let telemetry = TelemetryProbe::new(&simulator);
    let subscriptions = telemetry.observe(&simulator);

    let recording = telemetry.record_until_heartbeat(&mut simulator);

    assert_eq!(
        recording.samples.len(),
        13,
        "DATA frames before first heartbeat"
    );
    assert_eq!(recording.heartbeats.len(), 1, "first heartbeat missing");

    assert!(
        recording.samples[0].uptime_ms <= 1,
        "first DATA must be immediate"
    );

    for (index, sample) in recording.samples.iter().enumerate() {
        let (input_mv, expected_raw) = &ADC_INPUTS[index % ADC_INPUTS.len()];

        assert_eq!(sample.device_id, 0, "DATA device ID");
        assert_eq!(sample.pump, 0, "reported pump command");
        assert!(sample.raw <= 1023, "ADC reading must be 10-bit");
        assert!(
            expected_raw.contains(&sample.raw),
            "A0 at {input_mv} mV: {sample:?}"
        );
    }

    for pair in recording.samples.windows(2) {
        let elapsed_ms = pair[1].uptime_ms.wrapping_sub(pair[0].uptime_ms);

        assert!(
            (5000..=5001).contains(&elapsed_ms),
            "DATA interval: {elapsed_ms} ms"
        );
    }

    let heartbeat = &recording.heartbeats[0];

    assert_eq!(heartbeat.device_id, 0, "HB device ID");
    assert!(
        (60 * 1000..=60 * 1000 + 10).contains(&heartbeat.uptime_ms),
        "HB uptime: {}",
        heartbeat.uptime_ms
    );

    telemetry.assert_uart_complete();
    telemetry.assert_pump_stayed_off(&simulator);

    drop(subscriptions);
}
