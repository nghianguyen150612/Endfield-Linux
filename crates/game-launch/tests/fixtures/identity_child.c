#define _POSIX_C_SOURCE 200809L

#include <stdio.h>
#include <stdlib.h>
#include <time.h>

int main(int argc, char **argv) {
    printf("argc=%d\n", argc);
    for (int i = 0; i < argc; ++i) printf("arg%d=%s\n", i, argv[i]);
    printf("marker=%s\n", getenv("P034_MARKER") ? getenv("P034_MARKER") : "missing");
    const char *create_marker = getenv("P034_CREATE_MARKER");
    if (create_marker) {
        FILE *marker = fopen(create_marker, "w");
        if (marker) fclose(marker);
    }
    const char *delay_ms = getenv("P034_DELAY_MS");
    if (delay_ms) {
        unsigned long milliseconds = strtoul(delay_ms, NULL, 10);
        struct timespec delay = {
            .tv_sec = (time_t)(milliseconds / 1000),
            .tv_nsec = (long)(milliseconds % 1000) * 1000000L,
        };
        nanosleep(&delay, NULL);
    }
    const char *output_bytes = getenv("P034_OUTPUT_BYTES");
    if (output_bytes) {
        unsigned long count = strtoul(output_bytes, NULL, 10);
        for (unsigned long i = 0; i < count; ++i) {
            fputc('o', stdout);
            fputc('e', stderr);
        }
    }
    return 23;
}
