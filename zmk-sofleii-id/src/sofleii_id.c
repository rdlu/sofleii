/*
 * The sofleii identity record (#98). Layout, little-endian, 4-byte aligned:
 *
 *   char     magic[16]     "SOFLEII-ID/1", NUL-padded (the /1 is the layout)
 *   uint32_t reset         this image's reset handler (Thumb bit included)
 *   char     keyboard[16]  CONFIG_SOFLEII_ID_KEYBOARD, NUL-padded
 *   char     half[8]       CONFIG_SOFLEII_ID_HALF, NUL-padded
 *
 * `reset` is what makes the record trustworthy in CURRENT.UF2: the
 * bootloader only rewrites the pages a new image covers, so the tail of an
 * older, longer image (and its record) can outlive it. sofleii accepts a
 * record only if `reset` equals the vector table's reset entry at the start
 * of the application, i.e. the record belongs to the image that boots.
 */

#include <stdint.h>
#include <zephyr/init.h>
#include <zephyr/sys/util.h>

BUILD_ASSERT(sizeof(CONFIG_SOFLEII_ID_KEYBOARD) > 1 && sizeof(CONFIG_SOFLEII_ID_KEYBOARD) <= 16,
             "CONFIG_SOFLEII_ID_KEYBOARD must be 1..15 bytes");
BUILD_ASSERT(sizeof(CONFIG_SOFLEII_ID_HALF) > 1 && sizeof(CONFIG_SOFLEII_ID_HALF) <= 8,
             "CONFIG_SOFLEII_ID_HALF must be 1..7 bytes");

extern void z_arm_reset(void);

struct sofleii_id {
    char magic[16];
    uint32_t reset;
    char keyboard[16];
    char half[8];
} __aligned(4);

static const struct sofleii_id sofleii_id = {
    .magic = "SOFLEII-ID/1",
    .reset = (uint32_t)(uintptr_t)z_arm_reset,
    .keyboard = CONFIG_SOFLEII_ID_KEYBOARD,
    .half = CONFIG_SOFLEII_ID_HALF,
};

/* Nothing reads the record at runtime; this keeps --gc-sections from
 * dropping it. */
static int sofleii_id_keep(void) {
    (void)*(volatile const uint8_t *)&sofleii_id;
    return 0;
}

SYS_INIT(sofleii_id_keep, APPLICATION, 99);
