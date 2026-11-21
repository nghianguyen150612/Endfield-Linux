#define _POSIX_C_SOURCE 200809L
#include <errno.h>
#include <fcntl.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <sys/stat.h>
#include <time.h>
#include <unistd.h>

static int write_registry(const char *prefix, const char *name, const char *body) {
    char path[4096];
    int length = snprintf(path, sizeof(path), "%s/%s", prefix, name);
    if (length < 0 || (size_t)length >= sizeof(path)) {
        return -1;
    }
    int fd = open(path, O_WRONLY | O_CREAT | O_EXCL | O_CLOEXEC, 0600);
    if (fd < 0) {
        return -1;
    }
    size_t remaining = strlen(body);
    const char *cursor = body;
    while (remaining > 0) {
        ssize_t written = write(fd, cursor, remaining);
        if (written < 0) {
            int saved_errno = errno;
            close(fd);
            errno = saved_errno;
            return -1;
        }
        cursor += written;
        remaining -= (size_t)written;
    }
    return close(fd);
}

int main(int argc, char **argv) {
    const char *prefix = getenv("WINEPREFIX");
    const char *mode = argc > 1 ? argv[1] : "complete";
    if (prefix == NULL || prefix[0] != '/') {
        return 80;
    }
    if (strcmp(mode, "exit37") == 0) {
        return 37;
    }
    if (strcmp(mode, "sleep") == 0) {
        struct timespec delay = {.tv_sec = 5, .tv_nsec = 0};
        (void)nanosleep(&delay, NULL);
    }
    if (strcmp(mode, "noevidence") == 0) {
        return 0;
    }

    char drive_c[4096];
    char dosdevices[4096];
    char c_drive[4096];
    if (snprintf(drive_c, sizeof(drive_c), "%s/drive_c", prefix) >=
            (int)sizeof(drive_c) ||
        snprintf(dosdevices, sizeof(dosdevices), "%s/dosdevices", prefix) >=
            (int)sizeof(dosdevices) ||
        snprintf(c_drive, sizeof(c_drive), "%s/c:", dosdevices) >=
            (int)sizeof(c_drive)) {
        return 81;
    }
    if (mkdir(drive_c, 0700) != 0 || mkdir(dosdevices, 0700) != 0 ||
        symlink("../drive_c", c_drive) != 0) {
        return 82;
    }
    if (strcmp(mode, "partial-sleep") == 0) {
        struct timespec delay = {.tv_sec = 5, .tv_nsec = 0};
        (void)nanosleep(&delay, NULL);
    }
    if (write_registry(prefix, "system.reg",
                       "WINE REGISTRY Version 2\n\n[Software\\Wine\\System] 1234\n") != 0 ||
        write_registry(prefix, "user.reg",
                       "WINE REGISTRY Version 2\n\n[Software\\Wine\\User] 1234\n") != 0) {
        return 83;
    }
    return 0;
}
