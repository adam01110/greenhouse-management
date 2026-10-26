#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sim_avr.h>
#include <sim_elf.h>
#include <sim_io.h>
#include <avr_ioport.h>

static avr_t *cpu;
static uint64_t edges[3];
static unsigned edge_count;
static unsigned last_level;

static void fail(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(1);
}

static void led_changed(struct avr_irq_t *irq, uint32_t value, void *parameter) {
    (void)irq;
    (void)parameter;
    unsigned level = value & 1;
    if (level != last_level && edge_count < 3) {
        edges[edge_count++] = cpu->cycle;
    }
    last_level = level;
}

static void step(void) {
    int state = avr_run(cpu);
    if (state == cpu_Done || state == cpu_Crashed) {
        fail("Firmware stopped before completing the clock checks");
    }
}

static void check_edges(void) {
    uint64_t deadline = cpu->cycle + UINT64_C(40000000);
    while (edge_count < 3 && cpu->cycle < deadline) {
        step();
    }
    if (edge_count != 3) {
        fail("LED did not produce three transitions within 2.5 simulated seconds");
    }
    for (unsigned i = 1; i < 3; ++i) {
        uint64_t interval = edges[i] - edges[i - 1];
        printf("LED interval: %" PRIu64 " CPU cycles\n", interval);
        // Allow polling/ISR jitter, but reject OCR250's 64,000-cycle error.
        if (interval < UINT64_C(16000000) - 512 ||
            interval > UINT64_C(16000000) + 512) {
            fail("LED interval differs from one second by more than 32 us");
        }
    }
}

int main(int argc, char **argv) {
    if (argc != 2) {
        fprintf(stderr, "Usage: %s firmware.elf\n", argv[0]);
        return 2;
    }
    elf_firmware_t firmware = {0};
    if (elf_read_firmware(argv[1], &firmware) != 0) {
        fail("Cannot read firmware ELF");
    }
    cpu = avr_make_mcu_by_name("atmega328p");
    if (!cpu || avr_init(cpu) != 0) {
        fail("Cannot initialize ATmega328P simulation");
    }
    cpu->frequency = 16000000;
    avr_load_firmware(cpu, &firmware);
    avr_irq_t *led = avr_io_getirq(cpu, AVR_IOCTL_IOPORT_GETIRQ('B'), IOPORT_IRQ_PIN5);
    if (!led) {
        fail("Cannot observe Uno D13/PB5");
    }
    avr_irq_register_notify(led, led_changed, NULL);

    uint32_t counter_address = 0;
    unsigned counter_symbols = 0;
    for (uint32_t i = 0; i < firmware.symbolcount; ++i) {
        if (strstr(firmware.symbol[i]->symbol, "MILLIS")) {
            counter_address = firmware.symbol[i]->addr & 0xffff;
            ++counter_symbols;
        }
    }
    if (counter_symbols != 1 || counter_address < 0x100 ||
            counter_address + 3 > cpu->ramend) {
        fail("Expected one SRAM MILLIS counter symbol in the firmware ELF");
    }

    check_edges();
    // ATmega328P OCR0A, TCCR0A/B and TIMSK0: CTC, /64, compare-A only.
    if (cpu->data[0x47] != 249 || cpu->data[0x44] != 2 ||
            cpu->data[0x45] != 3 || cpu->data[0x6e] != 2) {
        fail("Unexpected Timer0 configuration");
    }

    // Pause outside the ISR/atomic read before injecting a near-rollover value.
    uint64_t deadline = cpu->cycle + 16000;
    while (!cpu->sreg[S_I] && cpu->cycle < deadline) {
        step();
    }
    if (!cpu->sreg[S_I]) {
        fail("Interrupts remained disabled for a full millisecond");
    }
    uint32_t near_wrap = UINT32_MAX - 499;
    for (unsigned i = 0; i < 4; ++i) {
        cpu->data[counter_address + i] = (uint8_t)(near_wrap >> (8 * i));
    }
    edge_count = 0;
    check_edges();

    deadline = cpu->cycle + 16000;
    while (!cpu->sreg[S_I] && cpu->cycle < deadline) {
        step();
    }
    if (!cpu->sreg[S_I]) {
        fail("Interrupts remained disabled after rollover");
    }
    uint32_t after_wrap = 0;
    for (unsigned i = 0; i < 4; ++i) {
        after_wrap |= (uint32_t)cpu->data[counter_address + i] << (8 * i);
    }
    if (after_wrap > 2000) {
        fail("Millisecond counter did not wrap as expected");
    }
    printf("Counter after rollover: %" PRIu32 " ms\n", after_wrap);
    puts("Timer0 tick, LED cadence and rollover checks passed");
    avr_terminate(cpu);
    return 0;
}
