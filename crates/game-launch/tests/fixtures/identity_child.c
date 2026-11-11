#include <stdio.h>
#include <stdlib.h>

int main(int argc, char **argv) {
    printf("argc=%d\n", argc);
    for (int i = 0; i < argc; ++i) printf("arg%d=%s\n", i, argv[i]);
    printf("marker=%s\n", getenv("P034_MARKER") ? getenv("P034_MARKER") : "missing");
    return 23;
}
