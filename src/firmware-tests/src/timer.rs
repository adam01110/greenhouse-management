use crate::Simulator;

mod probe;
use probe::TimerProbe;

const LED_INTERVAL_CYCLES: u64 = 16_000_000;
const ALLOWED_JITTER_CYCLES: u64 = 512;
const MILLIS_BEFORE_ROLLOVER: u32 = u32::MAX - 499;

#[test]
fn timer_tick_led_cadence_and_millis_rollover() {
    let mut simulator = Simulator::load();
    let timer = TimerProbe::new(&simulator);
    let _subscription = timer.observe(&simulator);

    let edges = timer.record_three_edges(&mut simulator);
    assert_timer_configuration(&simulator);
    assert_one_second_intervals(&edges, "before rollover");

    timer.set_millis(&mut simulator, MILLIS_BEFORE_ROLLOVER);
    let edges = timer.record_three_edges(&mut simulator);
    assert_one_second_intervals(&edges, "after rollover");

    let millis = timer.millis(&mut simulator);
    assert!(millis <= 2000, "counter did not wrap: {millis} ms");
}

fn assert_timer_configuration(simulator: &Simulator) {
    for (register, address, expected) in [
        ("OCR0A", 0x47, 249),
        ("TCCR0A", 0x44, 2),
        ("TCCR0B", 0x45, 3),
        ("TIMSK0", 0x6e, 2),
    ] {
        assert_eq!(simulator.read(address), expected, "Timer0 {register}");
    }
}

fn assert_one_second_intervals(edges: &[u64], phase: &str) {
    for pair in edges.windows(2) {
        let interval = pair[1] - pair[0];
        let jitter = interval.abs_diff(LED_INTERVAL_CYCLES);
        assert!(
            jitter <= ALLOWED_JITTER_CYCLES,
            "{phase}: LED interval {interval} cycles"
        );
    }
}
