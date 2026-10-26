#include <inttypes.h>
#include <stdint.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sim_avr.h>
#include <sim_elf.h>
#include <sim_io.h>
#include <avr_adc.h>
#include <avr_ioport.h>
#include <avr_uart.h>

static avr_t *cpu;
static avr_irq_t *adc_input;
static char line[40];
static unsigned used, data_frames, heartbeat_frames;
static uint32_t last_data_time;
static uint32_t input_mv;

static void fail(const char *message) {
    fprintf(stderr, "%s\n", message);
    exit(1);
}

static void adc_requested(struct avr_irq_t *irq, uint32_t value, void *parameter) {
    (void)irq;
    (void)value;
    (void)parameter;
    avr_raise_irq(adc_input, input_mv);
}

static void pump_changed(struct avr_irq_t *irq, uint32_t value, void *parameter) {
    (void)irq;
    (void)parameter;
    if (value & 1) {
        fail("Pump command went high without an agreed watering policy");
    }
}

static void uart_byte(struct avr_irq_t *irq, uint32_t value, void *parameter) {
    (void)irq;
    (void)parameter;
    if (used + 1 >= sizeof line) {
        fail("UART line exceeded expected maximum frame size");
    }
    line[used++] = (char)(value & 0xff);
    if ((value & 0xff) != '\n') {
        return;
    }
    line[used] = 0;
    if (used < 2 || line[used - 2] != '\r') {
        fail("Frame did not end with CRLF");
    }
    unsigned device, raw, pump;
    uint32_t uptime;
    int consumed = 0;
    if (sscanf(line, "DATA,%u,%" SCNu32 ",%u,%u\r\n%n",
               &device, &uptime, &raw, &pump, &consumed) == 4 &&
            consumed == (int)used) {
        if (device != 0 || pump != 0 || raw > 1023) {
            fail("Unexpected DATA identity, raw range or commanded pump state");
        }
        if (!data_frames && uptime > 1) {
            fail("Initial DATA was not sampled immediately after clock initialization");
        }
        unsigned phase = data_frames % 3;
        if ((phase == 0 && raw != 0) ||
            (phase == 1 && (raw < 511 || raw > 512)) ||
            (phase == 2 && raw != 1023)) {
            fail("DATA did not reflect the applied A0 voltage");
        }
        if (data_frames && (uptime - last_data_time < 5000 ||
                uptime - last_data_time > 5001)) {
            fail("DATA cadence differs from five seconds");
        }
        last_data_time = uptime;
        ++data_frames;
        input_mv = data_frames % 3 == 0 ? 0 : data_frames % 3 == 1 ? 2500 : 5000;
    } else if (sscanf(line, "HB,%u,%" SCNu32 "\r\n%n",
                      &device, &uptime, &consumed) == 2 && consumed == (int)used) {
        if (device != 0 || uptime < 60000 || uptime > 60010) {
            fail("Unexpected heartbeat identity or uptime");
        }
        ++heartbeat_frames;
    } else {
        fail("Unexpected/non-protocol UART line");
    }
    fputs(line, stdout);
    used = 0;
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
    cpu->vcc = cpu->avcc = cpu->aref = 5000;
    avr_load_firmware(cpu, &firmware);
    adc_input = avr_io_getirq(cpu, AVR_IOCTL_ADC_GETIRQ, ADC_IRQ_ADC0);
    avr_irq_t *trigger = avr_io_getirq(cpu, AVR_IOCTL_ADC_GETIRQ, ADC_IRQ_OUT_TRIGGER);
    avr_irq_t *uart = avr_io_getirq(cpu, AVR_IOCTL_UART_GETIRQ('0'), UART_IRQ_OUTPUT);
    avr_irq_t *pump = avr_io_getirq(cpu, AVR_IOCTL_IOPORT_GETIRQ('B'), IOPORT_IRQ_PIN0);
    if (!adc_input || !trigger || !uart || !pump) {
        fail("Cannot observe ADC0, USART0 and Uno D8/PB0");
    }
    avr_irq_register_notify(trigger, adc_requested, NULL);
    avr_irq_register_notify(uart, uart_byte, NULL);
    avr_irq_register_notify(pump, pump_changed, NULL);
    while (!heartbeat_frames && cpu->cycle < UINT64_C(992000000)) {
        int state = avr_run(cpu);
        if (state == cpu_Done || state == cpu_Crashed) {
            fail("Firmware stopped before its heartbeat");
        }
    }
    // PORTB0 and DDRB0 must both match an actively driven low pump command.
    if (heartbeat_frames != 1 || data_frames != 13 || used ||
            !(cpu->data[0x24] & 1) || (cpu->data[0x25] & 1)) {
        fail("Missing telemetry or pump output not held low");
    }
    printf("Verified %u DATA frames and %u heartbeat; pump stayed off\n",
           data_frames, heartbeat_frames);
    avr_terminate(cpu);
    return 0;
}
